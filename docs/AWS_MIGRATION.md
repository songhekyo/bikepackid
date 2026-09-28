# Migrasi ke AWS — Ancang-ancang

## Kenapa AWS (bukan Oracle/Azure/VPS lebih gede)

Ditimbang dari sisi teknis murni, VPS lebih gede (Nusa/Hetzner upgrade) atau Oracle Free Tier sebenarnya lebih murah/simpel buat sekadar belajar pola microservice/BFF/API gateway sebagai **konsep**. Tapi keputusan final tetap AWS, karena alasannya bukan teknis — **relevansi pasar kerja**: kebanyakan lowongan nyebut AWS spesifik, dan belajar Oracle/Azure duluan lalu pindah ke AWS pas butuh buat kerjaan itu bayar dua kali (waktu belajar ulang + biaya migrasi lagi). Trade-off yang diterima sadar: AWS lebih ribet & billing-nya kurang predictable dibanding VPS flat-rate — makanya seluruh doc ini fokus ke **cara nahan biaya tetap murah** sambil tetap dapet exposure AWS yang genuinely relevan buat CV.

Dua prinsip yang pegang sepanjang migrasi ini:
1. **Cost-safety net dulu, baru bikin resource apa pun** — bukan sebaliknya.
2. **App production (`bikepackid`) dan eksperimen belajar AWS dipisah sejauh mungkin** — biar gagal eksperimen gak bikin app yang lagi jalan buat pilot user ikut down/rusak.

## Fase 0 — Cost safety net (WAJIB sebelum resource apa pun dibuat)

- [x] **Akun AWS baru khusus project ini** — AWS versi "new experience" (Builder ID + Projects), project "Proof of Concept", akun `025452940943`.
- [x] **AWS Budgets** — budget bulanan + alert 50/80/100%.
- [x] **Zero-Spend Budget** — kedua budget udah dibikin.
- [x] **MFA di Builder ID** (`profile.aws.amazon.com`) + **IAM user `haska`** dibikin (AdministratorAccess attached langsung, bukan lewat "Copy permissions"). **Catatan**: MFA khusus level IAM user gak ketemu di UI baru ini (search global "mfa" cuma nunjukin service IAM + Credential report) — dianggap gak blocking, Builder ID MFA udah cukup buat sekarang.
- [ ] **Cost allocation tag** — belum sempet diterapin konsisten ke semua resource, nyusul kalau mau breakdown Cost Explorer per kategori.
- [ ] (Opsional) Virtual card dengan limit — belum dipasang, kredit $120 dianggap cukup sebagai backstop sementara.
- [x] Kebiasaan cek **Billing → Credits → "Total estimated amount used"** tiap minggu — dipilih ketimbang Cost Explorer polos, karena langsung nunjukin sisa kredit real-time (Cost Explorer sendiri delay ~24 jam).

## Fase 1 — Fundamental AWS, biaya nyaris nol

Tujuan fase ini: ngerti primitif dasar (IAM, VPC, Security Group, EC2) tanpa nyentuh servis yang mahal.

- [x] **VPC** — dipakai **default VPC** yang udah otomatis ada (`vpc-0794357d67a1e0167`, `172.31.0.0/16`, 3 subnet publik lintas AZ) ketimbang bikin custom — cukup buat pilot 1 instance, gak butuh isolasi network lebih ketat dari itu.
- [x] **Security Group** (`bikepackid`) — inbound `22`/`80`/`443` doang, SSH awalnya dibatasin ke "My IP" (lihat catatan SSM di bawah kenapa ini akhirnya dilepas).
- [x] **EC2 instance** — mulai dari `t4g.micro` (1GB, replikasi persis VPS lama), di-**upgrade ke `t4g.small`** (2GB) begitu database Postgres dipindah jalan lokal di instance yang sama (lihat Fase 2, `t4g.micro` gak cukup RAM buat backend+Postgres bareng).
- [x] **Elastic IP** — dibikin & di-associate, dipakai sebagai target A record `bikepacking.cyou`.
- [x] **Region**: `ap-southeast-2` Sydney (dipaksa AWS, lihat catatan di bawah), bukan Singapore kayak asumsi awal.

**Catatan biaya yang sering nyaris kepeleset — NAT Gateway**: kalau nanti mau subnet privat (best practice "beneran", instance app di privat subnet gak langsung ke-expose internet), itu butuh NAT Gateway buat akses keluar (misal `docker pull` dari GHCR). NAT Gateway itu ~$32+/bulan **cuma buat nyala**, di luar biaya data yang lewat situ — buat pilot skala kecil ini gak worth, mending public subnet + Security Group ketat dulu.

## Akses instance: SSH → AWS Systems Manager Session Manager

IP publik laptop yang dipakai buat SSH sering berubah (ISP dynamic IP, ganti WiFi/hotspot) — tiap kali berubah, rule "My IP" di security group jadi stale dan nge-block diri sendiri (`Connection established` di `ssh -v` tapi macet nunggu banner, atau langsung gagal). Ketimbang terus-terusan update rule manual, pindah ke **SSM Session Manager**: instance yang inisiasi koneksi keluar (outbound HTTPS) ke Systems Manager, bukan kamu yang connect masuk lewat port 22 — otentikasi pakai IAM, gak peduli IP publik kamu berapa/berubah kapan. Setup: IAM role (`bikepackid-ec2-ssm-role`, policy `AmazonSSMManagedInstanceCore`) di-attach ke instance, tunggu check-in pertama (butuh **reboot** kalau role di-attach setelah instance udah running — SSM Agent yang udah kadung start duluan gak otomatis re-auth). Setelah ini jalan, port 22 di security group bisa dicabut total — attack surface SSH publik hilang.

## Database: dari rencana "tetap Supabase" ke Postgres self-hosted

Rencana awal Fase 2 (lihat draft lama di bawah) nganggep Supabase tetap dipertahankan. Keputusan berubah pas eksekusi: Postgres dipindah jalan sebagai **container terpisah di instance EC2 yang sama** (`docker-compose.postgres.yml`, override khusus non-VPS), alasannya murni belajar cara jalanin Postgres sendiri (bukan managed service) sebagai bagian dari tujuan belajar AWS.

Konsekuensi teknis yang ketemu pas eksekusi (dicatat di `docs/INFRA_HISTORY.md` juga):
- **RAM**: alasan awal migrasi ke Supabase (dari VPS Nusa) itu karena 1GB RAM ketat — masalah yang sama muncul lagi di `t4g.micro` (juga 1GB). Fix: upgrade ke `t4g.small` (2GB).
- **Password Postgres**: `openssl rand -base64` menghasilkan karakter `+`/`/`/`=` yang bikin `DATABASE_URL` gagal di-parse sebagai URL (`InvalidPort`). Fix: pakai `openssl rand -hex` (cuma `0-9a-f`, aman langsung dipakai di URL).
- **Migrasi data dari Supabase**: `pg_dump -Fc` dari Supabase (Session pooler) → `pg_restore --no-owner --no-privileges` ke Postgres EC2. Gagal di percobaan pertama karena backend udah keburu auto-run migrasi `sqlx`-nya sendiri (bikin schema kosong duluan, termasuk foreign key constraint aktif) sebelum restore jalan — `pg_restore` yang nyoba `CREATE TABLE`/`COPY` ke schema yang udah ada constraint aktif gagal insert data karena urutan tabel gak dependency-aware. Fix: `DROP DATABASE` + `CREATE DATABASE` (beneran kosong tanpa schema) sebelum restore, biar `pg_restore` yang atur urutan create-schema → load-data → pasang-constraint dengan benar. Extension `supabase_vault` di dump gagal restore (gak ada di Postgres vanilla) — diabaikan, itu internal Supabase doang, gak dipakai aplikasi.

## Fase 2 — Deploy ulang arsitektur (BFF, gateway, microservice) — di sinilah belajar yang kamu incar

- [ ] **BFF** — Rust/Go/Node (pilih salah satu, gak usah semua) jalan sebagai EC2 instance/container terpisah dari backend utama. Ini juga jadi tempat natural buat nambahin dukungan **Bearer token** (App mobile gak bisa pakai cookie httpOnly kayak browser) sebagai jembatan ke backend Rust yang cookie-based.
- [ ] **AWS API Gateway** (produk asli, bukan self-hosted Kong/Traefik — karena tujuannya belajar nama produk ini spesifik) — taro di depan BFF/backend, coba fitur dasarnya: routing, rate limiting, request/response transformation. **Koreksi**: free tier "1 juta request/bulan gratis 12 bulan" cuma berlaku buat akun yang dibuat **sebelum Juli 2025** — akun baru (sesuai rencana Fase 0) dapetnya **kredit sekali doang** (lihat angka real di bawah), bukan free tier bulanan. Gak masalah di traffic pilot kita (lihat breakdown biaya di bawah) — HTTP API $1.00/juta request, buat ~10rb request/bulan itu ~$0.01/bulan.
- [ ] **ECS + Fargate** (container orchestration, versi "beneran" dari `docker compose` yang udah biasa dipakai) — **jangan mulai dari sini**, baru masuk setelah EC2+Docker Compose manual berasa udah nyaman. Fargate itu pay-per-resource (vCPU+RAM per detik container jalan) — predictable selama container-nya kecil & gak nyala 24/7 pas cuma eksperimen.

Urutan sengaja EC2-dulu-baru-ECS: biar kerasa bedanya "container biasa" vs "container yang di-orchestrate", bukan langsung lompat ke abstraksi tinggi tanpa ngerti yang di-abstraksi-in apa.

## Fase 3 — CI/CD & observability (opsional, bisa nunda)

- [x] Pipeline GHCR+Watchtower **jalan apa adanya** di EC2 — cuma satu penyesuaian yang gak kebayang dari awal: CI cuma pernah build image `amd64` (runner default GitHub Actions), sementara EC2 Graviton itu `arm64` — beda instruction set total, image `amd64` gagal total (`exec format error`), bukan cuma lambat. Fix awal: `docker/setup-qemu-action` (cross-build `arm64` di runner `amd64`) — **kelewat lambat** buat Rust (compiler itu syscall/thread-heavy, kasus terburuk buat emulasi: 18+ menit gak kelar). Fix final: matrix build di runner native masing-masing (`ubuntu-latest` buat `amd64`, `ubuntu-24.04-arm` buat `arm64`) + `docker buildx imagetools create` buat gabung jadi 1 manifest multi-arch — balik ke **arm64-only single job** begitu Nusa (satu-satunya konsumen `amd64`) di-decommission.
- [x] Grafana Cloud + Alloy jalan sama persis di EC2 — dikonfirmasi lewat log Alloy bersih (gak ada `Error:`) + trace baru kekonfirmasi masuk ke Grafana Cloud Explore setelah cutover. Gak ada yang perlu diubah dari sisi observability.
- **GHCR image sempat balik private** dua kali (recurring, kemungkinan besar karena setting "Inherit access from source repository" ke-reset tiap CI push) — bukan cuma soal Watchtower di VPS lama, `docker pull` manual di EC2 juga ikut kena `unauthorized`. Fix yang dipilih kali ini: **`docker login ghcr.io` pakai classic PAT** (`read:packages` scope) di instance EC2, bukan set-public-in lagi — lebih tahan lama karena gak bergantung ke toggle visibility yang keukur ke-reset sendiri. (Catatan: fine-grained PAT GitHub **belum support** GHCR/Packages sepenuhnya — harus classic token.)

## Estimasi biaya (region `ap-southeast-2` Sydney)

**Catatan**: AWS versi "new experience" (Builder ID + Projects) nge-lock region ke `ap-southeast-2` Sydney secara otomatis, gak bisa dipilih manual — jadi bukan `ap-southeast-1` Singapore kayak asumsi awal. Angka di bawah tetap dipakai sebagai estimasi (harga Sydney vs Singapore beda tipis, gak signifikan buat skala pilot ini), tinggal disesuaikan kalau nanti ada perbedaan harga nyata yang keliatan pas resource beneran jalan.

Dipisah antara yang **harus nyala 24/7** (backend buat pilot user) dan yang **cuma nyala pas lagi latihan** (BFF/gateway/Fargate) — nunjukin langsung dampak dari prinsip pemisahan production vs eksperimen di bawah.

**Selalu nyala:**

| Resource | Spek | Estimasi/bulan |
|---|---|---|
| EC2 `t4g.micro` (backend Rust) | 2 vCPU/1GB, ARM | ~$7.5-8 |
| EBS gp3 (root volume) | 20GB | ~$2 |
| Elastic IP | attached ke instance yang running | $0 |
| Data transfer out | JSON kecil, foto tetap lewat R2 bukan AWS | nyaris $0 |
| **Subtotal** | | **~$9.5-10/bulan (~Rp150.000-160.000)** |

**Cuma nyala pas latihan (BFF, API Gateway, Fargate):**

| Resource | Spek | Estimasi |
|---|---|---|
| EC2 `t4g.micro` (BFF) | sama kayak di atas, tapi nyala ~20 jam/bulan doang | ~$0.20/bulan (vs ~$7.5-8 kalau ikut 24/7) |
| AWS API Gateway (HTTP API) | $1.00/juta request | ~$0.01/bulan buat ~10rb request pilot |
| ECS Fargate (container eksperimen) | $0.04048/vCPU-jam + $0.004445/GB-jam (us-east-1, Singapore ~10-30% lebih) | ~$0.015/jam container nyala — sesi 5 jam ≈ $0.08 |

**Total realistis**: kalau BFF ikut nyala 24/7 bareng backend → **~$17-18/bulan (~Rp270.000-285.000)**. Kalau BFF cuma nyala pas latihan (sesuai prinsip di bawah) → **~$10-11/bulan (~Rp160.000-175.000)** — malah lebih murah dari VPS Nusa sekarang (~Rp100rb doang lebih murah dikit, tapi dapet exposure AWS yang jadi tujuan awal migrasi ini).

**Yang sengaja dihindarin** (kalau kepasang gak sengaja, bisa 3-4x lipatin tagihan):
- **NAT Gateway** — ~$32+/bulan cuma buat nyala, di luar biaya data.
- **Route 53** — gak perlu, DNS tetap di provider yang sekarang, tinggal ganti A record nunjuk ke IP EC2.

**Buffer (angka real, dicek langsung dari Billing → Credits)**: kredit **$120** — $100 dari "AWS Free Tier" + $20 bonus dari nyelesain task "Set up a cost budget using AWS Budgets" (Fase 0 di atas) — **expire 12 bulan dari signup** (bukan 6 bulan, koreksi dari estimasi awal). Karena jendela waktunya longgar, batasan yang beneran relevan itu **jumlah dollar terpakai**, bukan tanggal — cek "Total estimated amount used" di halaman Credits itu tiap minggu (bareng kebiasaan Cost Explorer). Rencana: kalau pakai `t4g.small` (~$17/bulan EC2+EBS), **terminate instance begitu kepakai udah nyampe ~$80-90** (nyisain buffer ~$30-40 buat eksperimen Fase 2) — kira-kira jatuh di bulan ke-5 dari sekarang, tapi angka aktual di Billing lebih akurat daripada patokan kalender.

## Prinsip hemat biaya (pegang terus sepanjang eksperimen)

- **EC2 dengan harga tetap per jam** buat servis yang emang harus nyala 24/7 (backend production buat pilot user) — predictable, gampang dihitung per bulan.
- **Matiin (stop, bukan cuma biarin) instance eksperimen** begitu selesai sesi belajar — EC2 cuma nge-charge compute selama **running**, instance yang di-stop cuma kena biaya storage EBS-nya doang (kecil, ~$0.08/GB/bulan).
- **Instance ARM (`t4g`) lebih murah dari x86 (`t3`)** buat spek setara — dipakai di mana pun ARM bisa dipakai.
- **Hindari NAT Gateway** kecuali beneran perlu (lihat Fase 1).
- **Pisah "yang harus hidup 24/7 buat pilot user" dari "yang cuma buat eksperimen belajar"** — biar yang eksperimen (ECS Fargate latihan, API Gateway coba-coba) bisa dibongkar-pasang bebas tanpa mikirin app production ikut kena efek.
- **Pakai Terraform buat resource yang sering dibongkar-pasang** (lihat catatan Terraform R2 yang sempat ditunda di `JOURNEY_TODO.md` Task 0 — ini momen yang pas buat diambil lagi, khusus buat resource AWS eksperimen) — `terraform destroy` abis sesi belajar, `terraform apply` lagi pas mau lanjut, biaya cuma kena pas resource-nya beneran nyala.

## Status: cutover selesai

Semua pertanyaan di fase perencanaan awal ini udah kejawab lewat eksekusi:

- ~~Region AWS mana~~ — `ap-southeast-2` Sydney (dipaksa AWS, gak bisa dipilih manual).
- ~~Apa domain langsung dipindah atau paralel dulu~~ — **paralel dulu**: EC2 disetup & ditest lengkap (health check, migrasi data, login OAuth) selagi Nusa masih live, DNS TTL diturunin ke 60 detik dulu, baru A record `bikepacking.cyou` di-switch begitu EC2 confirmed jalan. Nusa di-cancel setelah beberapa hari monitoring stabil.
- ~~Supabase tetap dipertahankan atau pindah ke Postgres beneran~~ — **pindah ke Postgres self-hosted** di container EC2 yang sama (lihat bagian "Database" di atas), bukan RDS — konsisten sama prinsip belajar "jalanin sendiri", RDS jadi latihan terpisah kapan-kapan kalau mau.

Sisa kerjaan lanjut ke **Fase 2** (BFF, API Gateway, ECS Fargate) sebagai eksperimen belajar terpisah dari production yang sekarang udah stabil di EC2.
