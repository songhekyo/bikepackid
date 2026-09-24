# Migrasi ke AWS — Ancang-ancang

## Kenapa AWS (bukan Oracle/Azure/VPS lebih gede)

Ditimbang dari sisi teknis murni, VPS lebih gede (Nusa/Hetzner upgrade) atau Oracle Free Tier sebenarnya lebih murah/simpel buat sekadar belajar pola microservice/BFF/API gateway sebagai **konsep**. Tapi keputusan final tetap AWS, karena alasannya bukan teknis — **relevansi pasar kerja**: kebanyakan lowongan nyebut AWS spesifik, dan belajar Oracle/Azure duluan lalu pindah ke AWS pas butuh buat kerjaan itu bayar dua kali (waktu belajar ulang + biaya migrasi lagi). Trade-off yang diterima sadar: AWS lebih ribet & billing-nya kurang predictable dibanding VPS flat-rate — makanya seluruh doc ini fokus ke **cara nahan biaya tetap murah** sambil tetap dapet exposure AWS yang genuinely relevan buat CV.

Dua prinsip yang pegang sepanjang migrasi ini:
1. **Cost-safety net dulu, baru bikin resource apa pun** — bukan sebaliknya.
2. **App production (`bikepackid`) dan eksperimen belajar AWS dipisah sejauh mungkin** — biar gagal eksperimen gak bikin app yang lagi jalan buat pilot user ikut down/rusak.

## Fase 0 — Cost safety net (WAJIB sebelum resource apa pun dibuat)

- [ ] **Akun AWS baru khusus project ini** — jangan gabung ke akun lain kalau ada. Isolasi biaya total.
- [ ] **AWS Budgets** — set budget bulanan (misal $25), alert di 50%/80%/100% terpakai, kirim ke email.
- [ ] **Zero-Spend Budget** — budget kedua yang alert begitu ada **spend sama sekali** (bukan nunggu ke threshold) — buat nangkep resource yang gak sengaja kebuat/lupa dimatiin secepat mungkin.
- [ ] **MFA di root account**, terus **jangan pernah dipakai buat kerja harian** — bikin IAM user terpisah (dengan MFA juga) buat operasional sehari-hari. Ini juga sekalian latihan konsep IAM yang genuinely dipakai di kerjaan.
- [ ] **Cost allocation tag** — semua resource yang dibuat dikasih tag `project=bikepackid` (atau `purpose=learning` buat yang eksperimen doang) dari awal, biar Cost Explorer bisa breakdown per kategori nanti.
- [ ] (Opsional, backstop paling keras) **Virtual card dengan limit** khusus buat akun AWS ini — kalau limitnya kelewat, charge ditolak duluan sebelum AWS nagih lebih jauh.
- [ ] Jadwal cek manual **Cost Explorer tiap minggu** selama bulan pertama — jangan cuma ngandelin alert, sambil belajar baca cost breakdown-nya juga.

## Fase 1 — Fundamental AWS, biaya nyaris nol

Tujuan fase ini: ngerti primitif dasar (IAM, VPC, Security Group, EC2) tanpa nyentuh servis yang mahal.

- [ ] **VPC sederhana** — 1 VPC, subnet publik doang dulu (skip subnet privat + NAT Gateway, lihat catatan biaya di bawah).
- [ ] **Security Group** — setara firewall di VPS lama, tapi per-instance/per-service, bukan satu firewall besar. Latihan scope serapat mungkin (cuma port yang beneran dipakai).
- [ ] **EC2 instance pertama** (`t4g.micro`, ARM Graviton — lebih murah dari x86, dan Rust cross-compile ke `aarch64` gak susah) — pindahin backend Rust yang ada, replikasi persis setup VPS lama dulu (jangan sambil belajar hal baru, biar ada baseline yang jalan).
- [ ] **Elastic IP** (opsional) — biar IP publik EC2 gak berubah tiap restart instance, penting banget begitu udah pasang DNS ke situ (Elastic IP gratis **selama** attached ke instance yang running — begitu instance mati/IP-nya nganggur, mulai kena charge — jangan dibiarin nganggur).

**Catatan biaya yang sering nyaris kepeleset — NAT Gateway**: kalau nanti mau subnet privat (best practice "beneran", instance app di privat subnet gak langsung ke-expose internet), itu butuh NAT Gateway buat akses keluar (misal `docker pull` dari GHCR). NAT Gateway itu ~$32+/bulan **cuma buat nyala**, di luar biaya data yang lewat situ — buat pilot skala kecil ini gak worth, mending public subnet + Security Group ketat dulu.

## Fase 2 — Deploy ulang arsitektur (BFF, gateway, microservice) — di sinilah belajar yang kamu incar

- [ ] **BFF** — Rust/Go/Node (pilih salah satu, gak usah semua) jalan sebagai EC2 instance/container terpisah dari backend utama. Ini juga jadi tempat natural buat nambahin dukungan **Bearer token** (App mobile gak bisa pakai cookie httpOnly kayak browser) sebagai jembatan ke backend Rust yang cookie-based.
- [ ] **AWS API Gateway** (produk asli, bukan self-hosted Kong/Traefik — karena tujuannya belajar nama produk ini spesifik) — taro di depan BFF/backend, coba fitur dasarnya: routing, rate limiting, request/response transformation. **Koreksi**: free tier "1 juta request/bulan gratis 12 bulan" cuma berlaku buat akun yang dibuat **sebelum Juli 2025** — akun baru (sesuai rencana Fase 0) dapetnya **kredit $200 sekali doang**, bukan free tier bulanan. Gak masalah di traffic pilot kita (lihat breakdown biaya di bawah) — HTTP API $1.00/juta request, buat ~10rb request/bulan itu ~$0.01/bulan.
- [ ] **ECS + Fargate** (container orchestration, versi "beneran" dari `docker compose` yang udah biasa dipakai) — **jangan mulai dari sini**, baru masuk setelah EC2+Docker Compose manual berasa udah nyaman. Fargate itu pay-per-resource (vCPU+RAM per detik container jalan) — predictable selama container-nya kecil & gak nyala 24/7 pas cuma eksperimen.

Urutan sengaja EC2-dulu-baru-ECS: biar kerasa bedanya "container biasa" vs "container yang di-orchestrate", bukan langsung lompat ke abstraksi tinggi tanpa ngerti yang di-abstraksi-in apa.

## Fase 3 — CI/CD & observability (opsional, bisa nunda)

- [ ] Pipeline GHCR+Watchtower yang udah ada **tetep bisa jalan apa adanya** di EC2 (Watchtower gak peduli VM-nya di mana) — gak wajib buru-buru ganti ke CodePipeline/CodeBuild. Migrasi ke situ jadi latihan terpisah kapan-kapan, bukan blocker migrasi awal.
- [ ] Grafana Cloud + Alloy juga jalan sama persis di EC2 kayak di VPS lama — gak ada yang perlu diubah di sisi ini.

## Estimasi biaya (region `ap-southeast-1` Singapore)

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

**Buffer**: kredit $200 sekali dari akun baru (lihat catatan di Fase 2) nutup eksperimen berbulan-bulan sebelum biaya asli mulai kepakai.

## Prinsip hemat biaya (pegang terus sepanjang eksperimen)

- **EC2 dengan harga tetap per jam** buat servis yang emang harus nyala 24/7 (backend production buat pilot user) — predictable, gampang dihitung per bulan.
- **Matiin (stop, bukan cuma biarin) instance eksperimen** begitu selesai sesi belajar — EC2 cuma nge-charge compute selama **running**, instance yang di-stop cuma kena biaya storage EBS-nya doang (kecil, ~$0.08/GB/bulan).
- **Instance ARM (`t4g`) lebih murah dari x86 (`t3`)** buat spek setara — dipakai di mana pun ARM bisa dipakai.
- **Hindari NAT Gateway** kecuali beneran perlu (lihat Fase 1).
- **Pisah "yang harus hidup 24/7 buat pilot user" dari "yang cuma buat eksperimen belajar"** — biar yang eksperimen (ECS Fargate latihan, API Gateway coba-coba) bisa dibongkar-pasang bebas tanpa mikirin app production ikut kena efek.
- **Pakai Terraform buat resource yang sering dibongkar-pasang** (lihat catatan Terraform R2 yang sempat ditunda di `JOURNEY_TODO.md` Task 0 — ini momen yang pas buat diambil lagi, khusus buat resource AWS eksperimen) — `terraform destroy` abis sesi belajar, `terraform apply` lagi pas mau lanjut, biaya cuma kena pas resource-nya beneran nyala.

## Belum diputuskan / didiskusikan lebih lanjut

- Region AWS mana (latency ke Indonesia — kemungkinan `ap-southeast-1` Singapore, sama kayak pertimbangan provider lain).
- Apa `bikepacking.cyou` (domain yang udah jalan) langsung dipindah pas migrasi, atau ada periode paralel VPS lama + AWS baru buat mastiin stabil dulu sebelum cutover.
- Kapan/apa Postgres (Supabase) tetap dipertahankan sebagai managed service eksternal, atau di titik tertentu juga dipindah ke RDS (buat belajar RDS) — belum ada keputusan.
