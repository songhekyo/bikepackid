# Riwayat infra — dari kosong sampai live

Catatan kronologis kenapa infra `bikepackid` bentuknya kayak sekarang. Bukan panduan langkah-demi-langkah (itu di [`auth-service/DEPLOY.md`](../auth-service/DEPLOY.md), historis, atau [`auth-service/DEPLOY_AWS.md`](../auth-service/DEPLOY_AWS.md), aktif — `auth-service/` dulu namanya `backend/`) — ini dokumentasi **keputusan dan perubahan**, termasuk yang rencana awalnya beda dari yang akhirnya kepake, dan kenapa.

Periode: 14–20 September 2026. Live pertama kali di `https://bikepacking.cyou`, 19 September 2026 (login Google end-to-end terkonfirmasi jalan lewat log).

## 1. Kenapa VPS mentah, bukan PaaS

Opsi awal yang dipertimbangkan: Railway/Render/Fly (PaaS, tinggal deploy). Dipilih VPS mentah sebagai gantinya karena tujuannya bukan cuma "aplikasi jalan", tapi juga buat belajar sistem engineering & networking langsung — SSH, firewall, reverse proxy, Docker, TLS — hal-hal yang di-abstract-kan habis oleh PaaS.

## 2. Provider: dari rencana Hetzner ke Nusa

Rencana awal: Hetzner CX22 (2 vCPU / 4GB RAM, ~Rp75-80rb/bulan) — dipilih karena harga jelas, dokumentasi bagus, dan cukup nyaman buat compile Rust + Postgres + reverse proxy jalan bareng.

Yang akhirnya kepake: VPS dari **Nusa** (provider Indonesia), **1 vCPU / 1GB RAM / 25GB disk**, region Jakarta, Fedora 42. Jauh lebih kecil dari rencana — RAM-nya di bawah bahkan tier paling murah yang tadinya dipertimbangkan. Ini konsekuensi yang harus ditangani di beberapa langkah berikutnya (swap, pemilihan database, dst), bukan diabaikan.

## 3. Validasi resource sebelum percaya rule-of-thumb

Daripada nebak "cukup gak sih 1GB buat compile Rust", di-benchmark dulu pakai simulasi lokal di laptop (Docker, bukan di server beneran):
```bash
DOCKER_BUILDKIT=0 docker build --memory=1g --memory-swap=1g -t bikepackid-test .
```
Hasilnya: **berhasil** dalam batas 1GB tanpa swap sama sekali (dependency graph proyek ini, per commit waktu itu, masih muat). Ini prinsip capacity-planning: ukur pakai angka nyata proyek sendiri, bukan cuma pakai rule of thumb generik ("Rust butuh ~2GB").

Meski hasil tes lokal itu positif, **swap tetap dipasang** di server beneran sebagai jaring pengaman murah (RAM sistem beneran juga dipakai OS/Docker daemon/dst yang gak ikut ter-simulasi di tes lokal itu) — bukan berdasarkan tes ini doang.

## 4. Setup server: swap, firewall, Docker

- **Swap 2GB** ditambahkan (`fallocate` + `mkswap` + `swapon`, persisted lewat `/etc/fstab`) — langkah yang gak ada di draft `DEPLOY.md` paling awal, ditambahkan setelah sadar RAM aktual jauh lebih kecil dari rencana.
- **`firewalld` ternyata tidak pre-installed** di image Fedora 42 dari Nusa ini — beda dari asumsi awal ("biasanya udah aktif default di image cloud"). Diinstall manual sebelum dikonfigurasi.
- **Docker** diinstall via script resmi `get.docker.com` — Fedora terdeteksi otomatis, Engine + Compose plugin sekaligus.
- **Build pertama sempat gagal** karena `docker compose up -d --build` dijalankan langsung di shell SSH (bukan di `tmux`) — begitu koneksi SSH putus di tengah build (~9 menit jalan), BuildKit ikut nge-cancel build-nya (beda dari builder klasik, BuildKit nempel ke sesi client yang mulai). Setelah itu, semua build dijalankan di dalam `tmux` supaya tahan disconnect.

## 5. Database: dari Postgres lokal ke Supabase

Rencana awal & implementasi pertama `docker-compose.yml`: Postgres jalan sebagai container terpisah di VPS yang sama (`postgres:16-bookworm`, service `postgres`, volume lokal).

Diganti ke **Supabase** (managed Postgres) supaya VPS 1GB gak perlu nanggung beban Postgres sekaligus (bebasin ~100-250MB RAM yang tadinya kepake Postgres lokal) — keputusan yang langsung nyambung ke keterbatasan RAM di poin 2.

Migrasi ini sempat konflik dengan commit lain (`f6610da "dockers"`) yang di-push langsung ke `main` di luar PR — commit itu independen nambahin `Dockerfile`/`docker-compose.yml` versi lain yang masih pakai Postgres lokal, dan Dockerfile-nya sendiri **gak lengkap** (gak install `pkg-config`/`libssl-dev`, jadi bakal gagal build karena `sqlx`/`reqwest` butuh itu buat compile TLS). Konflik di-resolve dengan mempertahankan versi Supabase (lebih sesuai constraint RAM) dan Dockerfile yang udah teruji jalan.

**Isu tak terduga**: koneksi langsung (`db.<ref>.supabase.co:5432`) Supabase defaultnya **IPv6-only**. Container Docker di network default biasanya cuma dikasih IPv4 (walau host-nya sendiri punya alamat IPv6) — hasilnya `Network is unreachable` waktu backend coba connect. Dikonfirmasi lewat `getent ahosts` (cuma keluar alamat IPv6, gak ada IPv4). Fix: pakai **Session pooler** Supabase (bukan Direct connection, bukan Transaction pooler) — alternatif IPv4 yang justru direkomendasikan Supabase sendiri buat pola koneksi persisten/long-lived kayak yang dipakai `sqlx::PgPoolOptions` di backend ini.

## 6. Reverse proxy: dari rencana Caddy ke nginx

Rencana awal: **Caddy** — dipilih karena auto-HTTPS tanpa konfigurasi tambahan (`deploy/Caddyfile` sempat dibikin). Diganti ke **nginx** di tengah jalan — nginx lebih umum dipakai industri jadi lebih worth dipelajari, meski butuh langkah tambahan (`certbot`) buat TLS yang di Caddy otomatis.

Kendala yang ketemu & di-resolve pas setup nginx:
- **Default server block bawaan nginx** (`/etc/nginx/nginx.conf`, listen 80) bentrok sama config reverse-proxy baru yang juga listen di port 80 — di-comment-out.
- **SELinux** (Fedora, *enforcing* default) nolak nginx bikin koneksi keluar ke backend (`502 Bad Gateway` walau config udah benar) — fix: `setsebool -P httpd_can_network_connect on`.
- **Header `X-Forwarded-For`** di-set eksplisit ke `$remote_addr` (bukan `$proxy_add_x_forwarded_for` yang lazim di tutorial umum) — supaya nginx **overwrite** total header itu, bukan nambahin ke nilai yang mungkin dikirim klien. Backend pakai header ini buat rate limiting per-IP (`tower_governor`); kalau nginx cuma nambahin, klien jahat berpotensi ngirim `X-Forwarded-For` palsu buat coba bypass rate limiter.

## 7. Domain & TLS

Domain `bikepacking.cyou` diarahkan ke IP VPS (A record), lalu `certbot --nginx -d bikepacking.cyou` — otomatis dapat sertifikat Let's Encrypt dan nge-edit config nginx buat nambahin block HTTPS + redirect HTTP→HTTPS.

## 8. Google OAuth Console

Kesalahan awal yang ke-catch sebelum sempat dites: redirect URI yang mau didaftarkan di Google Console sempat ditulis `/api/auth/callback/google` — itu konvensi **NextAuth.js** (framework beda), bukan route yang beneran ada di backend Rust ini (`/auth/google/callback`). Dikoreksi sebelum didaftarkan ke Google Console, jadi gak sempat nyebabin kegagalan nyata di production.

## 9. Observability: dari "wiring doang" ke beneran kekirim (Grafana Cloud + Alloy)

Kode `telemetry.rs` udah siap OTel sejak awal, tapi belum ada tujuan (`OTEL_EXPORTER_OTLP_ENDPOINT` gak di-set). Diisi belakangan, pakai **Grafana Cloud** (free tier) + **Grafana Alloy** sebagai collector lokal (backend → Alloy tanpa auth di jaringan Docker → Alloy forward ke Grafana Cloud pakai kredensial akun). Setup ini kena **tiga bug berlapis**, masing-masing ke-tutup sama bug berikutnya — dokumentasi ini biar ke depan gak perlu re-diagnose dari nol kalau muncul lagi:

1. **Blocking reqwest client di runtime async.** `opentelemetry-otlp` tanpa feature eksplisit diam-diam resolve ke `reqwest-blocking-client`, bukan versi async — dipanggil dari dalam `#[tokio::main]`, gagal cepat dengan pesan generik `"network error"`. Ketauan dari `reqwest::blocking::wait` yang muncul di log pas `RUST_LOG` dinaikin ke `reqwest=trace`. Fix: `opentelemetry-otlp` di-pin eksplisit `default-features = false, features = ["http-proto", "reqwest-client", "trace"]`.
2. **Batch processor gak punya reactor Tokio.** Setelah fix #1, muncul panic baru: `there is no reactor running`. `SdkTracerProviderBuilder::with_batch_exporter()` (default) jalanin loop export-nya di `std::thread` polos, gak nempel ke runtime Tokio — cocok buat exporter blocking, tapi exporter kita sekarang async, butuh reactor buat `.await`. Ketauan dari baca source `opentelemetry_sdk` langsung (ada dua implementasi processor: `trace/span_processor.rs` yang pakai `std::thread`, dan `trace/span_processor_with_async_runtime.rs` yang pakai runtime beneran). Fix: pakai `span_processor_with_async_runtime::BatchSpanProcessor::builder(exporter, runtime::Tokio)`, attach via `.with_span_processor()`, plus enable feature `experimental_trace_batch_span_processor_with_async_runtime`.
3. **POST ke path yang salah.** Setelah fix #2, panic-nya hilang tapi export masih gagal, balik ke `"network error"` generik tanpa detail tambahan biarpun `hyper`/`reqwest` di-set ke `trace`. Diisolasi manual: `curl POST http://alloy:4318/v1/traces` dari container lain di network yang sama → `200 OK`; `curl GET http://alloy:4318/` (root) → `404`. Baca source `opentelemetry-otlp` (`resolve_http_endpoint`) konfirmasi: `.with_endpoint(...)` (jalur programatik, yang dipakai kode kita) dipakai **apa adanya**, `/v1/traces` **tidak** auto-ditambahin — beda dari jalur env var `OTEL_EXPORTER_OTLP_ENDPOINT` yang justru auto-nambahin. Backend selama ini POST ke `http://alloy:4318` (404), bukan `http://alloy:4318/v1/traces` (200). Fix: tambahin `/v1/traces` eksplisit di kode.

Pola diagnosis yang kepake konsisten di ketiga bug ini: **jangan percaya pesan error generik dari SDK** (`"network error"` itu sama sekali gak jelas), **isolasi tiap hop manual** (`curl` dari container terpisah buat mastiin jaringan Docker/Alloy gak salah), dan **baca source crate langsung** kalau dokumentasi/pesan error gak cukup (crate-nya udah ke-download di `~/.cargo/registry`, gratis dibaca) — bukan nebak-nebak nama feature/API berkali-kali.

Setelah tiga fix ini, sekaligus cleanup: `otelcol.exporter.debug` (exporter debug sementara yang dipasang buat diagnosis #2 dan #3) dan flag `--stability.level=experimental` yang dia butuhin, dicabut lagi begitu trace udah kekonfirmasi masuk ke Grafana Cloud.

## 10. Verifikasi live

Setelah semua di atas jalan, dites dari luar server (laptop, bukan `curl` di server sendiri):
```
GET /auth/google/login  → 303 ke accounts.google.com (PKCE + state ada)
GET /auth/google/callback → 303 (redirect sukses, latency ~2s buat tukar code + fetch profil + upsert user)
```
Dikonfirmasi lewat `docker compose logs -f backend` (structured JSON log, `request_id` per request) berbarengan sama tes login manual di browser.

## 11. Uptime monitoring

**UptimeRobot** ping `https://bikepacking.cyou/health` tiap 5 menit dari luar (bukan dari VPS sendiri), kirim alert email kalau gagal. Dipilih `/health` (bukan `/`) karena endpoint itu beneran nge-ping database, bukan 200 statis — jadi kedeteksi juga kalau Supabase yang bermasalah, bukan cuma proses backend crash. Setup-nya di luar codebase (dashboard UptimeRobot), dicatat di sini biar ke-track sebagai bagian dari infra.

## 12. Migrasi VPS Nusa → AWS EC2 (Graviton)

Periode: 24–28 September 2026. Detail rencana & alasan lengkap ada di [`docs/AWS_MIGRATION.md`](./AWS_MIGRATION.md) — bagian ini fokus ke bug/keputusan yang ketemu pas eksekusi, pola yang sama kayak section-section sebelumnya.

**Arsitektur CPU jadi masalah nyata, bukan cuma teori.** EC2 dipilih ARM (`t4g`, Graviton — lebih murah dari x86 `t3`), tapi CI (`ci.yml`) dari awal cuma pernah build image `amd64` (default runner GitHub Actions). Pull ke instance ARM gagal total dengan `exec format error` — beda instruction set, bukan sekadar kompatibilitas versi. Fix pertama (`docker/setup-qemu-action`, cross-build `arm64` di runner `amd64`) kelewat lambat buat Rust — compiler itu beban kerja paling parah kena penalty emulasi (18+ menit satu run, gak kelar-kelar). Fix final: **native runner per arsitektur** (`ubuntu-24.04-arm` buat `arm64`, bukan emulasi) via matrix build + `docker buildx imagetools create` buat gabung manifest — begitu Nusa (satu-satunya konsumen `amd64`) di-decommission, disederhanain lagi balik ke satu job `arm64`-only.

**Database: sejarah berulang, RAM lagi.** Alasan awal pindah dari Postgres lokal ke Supabase (section 5) itu RAM VPS 1GB. Keputusan Fase 2 migrasi AWS ini justru **balik lagi ke Postgres self-hosted** (container terpisah di EC2, `docker-compose.postgres.yml`) — sengaja, buat belajar jalanin Postgres sendiri, bukan lupa pelajaran lama. Konsekuensinya sama persis kayak dulu: `t4g.micro` (1GB) gak cukup nampung backend+Postgres bareng, upgrade ke `t4g.small` (2GB).

**Migrasi data Supabase → Postgres self-hosted, gagal-lalu-berhasil.** `pg_dump -Fc` dari Supabase, `pg_restore --no-owner --no-privileges` ke Postgres EC2. Percobaan pertama gagal 43 error — root cause: backend sempat jalan duluan (auto-run migrasi `sqlx`-nya sendiri) sebelum restore, jadi target database udah punya schema **dengan foreign key constraint aktif** pas `pg_restore` nyoba `COPY` data — beda dari restore ke database kosong biasa (constraint baru dipasang belakangan di post-data section, jadi urutan insert data gak masalah). Fix: `DROP DATABASE` + `CREATE DATABASE` (beneran kosong, gak ada schema sama sekali) sebelum restore ulang, biar `pg_restore` yang atur urutan create-schema → load-data → pasang-constraint sendiri.

**GHCR image balik private berulang kali** — kemungkinan besar setting "Inherit access from source repository" ke-reset tiap CI push, ngerusak baik Watchtower (VPS lama) maupun `docker pull` manual (EC2). Fix kali ini: `docker login` pakai **classic PAT** (`read:packages`) di instance, bukan gantungin ke toggle visibility yang keukur reset sendiri. Ketemu juga limitasi: fine-grained PAT GitHub belum support GHCR/Packages, harus classic token.

**Akses SSH tumbang berulang karena IP dinamis.** Rule security group `"My IP"` itu snapshot sesaat — ISP/jaringan Indonesia sering ganti IP publik (apalagi kalau pindah WiFi↔hotspot), bikin rule stale dan nge-block sendiri (gejalanya `ssh -v` macet di `Connection established`, gak pernah dapet banner balik). Pindah ke **AWS Systems Manager Session Manager** — instance yang konek keluar (outbound HTTPS) ke Systems Manager, otentikasi IAM, gak peduli IP client. Port 22 di security group akhirnya bisa dicabut total.

**Cutover domain paralel, bukan big-bang.** DNS TTL `bikepacking.cyou` diturunin ke 60 detik dulu, EC2 disetup & ditest lengkap (data ter-migrasi, login OAuth jalan) **sambil Nusa masih live**, baru A record di-switch. Karena domain gak berubah, gak perlu sentuh apa pun di Google OAuth Console (redirect URI tetap valid). Nusa dibiarin idle beberapa hari sebagai fallback sebelum beneran di-cancel.

## 13. Watchtower diam-diam berhenti update total selama berminggu-minggu

Ketemu 4 Oktober 2026, gak sengaja — lagi ngurusin fitur lain (AWS SES, waitlist endpoint), eh `/api/waitlist` 404 di production padahal kodenya udah lama di-merge ke `main`. Dicek `/version` endpoint: container `auth-service` masih jalan commit `6524e3e` (PR #32) — **27 commit di belakang `main`**, dari sebelum Journey Equipment/Sponsors, landing page, SES, waitlist, semuanya. `docker ps` nunjukin container "Up 24 hours", nyesatin — itu cuma restart (reboot EC2 atau semacamnya), bukan hasil pull image baru.

Root cause-nya ternyata sambungan langsung ke insiden section 12 ("GHCR image balik private berulang kali"): fix waktu itu `docker login` pakai classic PAT **di host** doang. Watchtower jalan di container terpisah — `docker.sock` yang di-mount cuma ngasih dia akses ke Docker Engine API host, **bukan** kredensial registry yang tersimpan di situ. Watchtower bikin request API registry sendiri (buat cek digest sebelum mutusin perlu pull atau enggak), dan itu butuh `config.json`-nya sendiri — gak pernah di-mount. Hasilnya: tiap poll (5 menit sekali, berminggu-minggu) gagal diam-diam dengan `401 Unauthorized`, log-nya cuma kebaca kalau dicek manual (`docker logs backend-watchtower-1`), gak ada notifikasi apapun secara default.

Yang bikin ini gampang kelewat: `docker pull` manual di host **selalu berhasil** (daemon host punya PAT-nya dari fix section 12) — jadi "coba manual, kok jalan" gak pernah membuktikan Watchtower-nya juga jalan, dua proses yang beda biarpun share Docker socket yang sama.

Fix: mount `${HOME}/.docker/config.json:/config.json:ro` ke container Watchtower juga (`auth-service/docker-compose.yml`), plus workaround manual (`docker compose pull && up -d`) buat langsung nyamain production ke `main` tanpa nunggu fix ini ke-apply duluan.

**Pelajaran buat ke depan**: Watchtower "container-nya nyala dan healthy" itu gak sama dengan "updatenya jalan" — perlu dicek `docker logs` beneran secara berkala, bukan cuma `docker ps`. Pertimbangkan nambahin notifikasi Watchtower (Slack/webhook) kalau project ini tumbuh, biar kegagalan diam-diam kayak gini kekirim otomatis, bukan nunggu ketemu gak sengaja.

## Ringkasan: rencana vs. kenyataan

| Komponen | Rencana awal | Yang kepake |
|---|---|---|
| Provider VPS | Hetzner CX22 (4GB RAM) | Nusa (1GB RAM) → **AWS EC2 Graviton** (`t4g.small`, 2GB) |
| Reverse proxy | Caddy (auto-TLS) | nginx + certbot |
| Database | Postgres lokal (container) | Supabase (Session pooler) → **Postgres self-hosted lagi** (container EC2) |
| Firewall | Asumsi udah ada | `firewalld` diinstall manual (Nusa) → AWS Security Group + **SSM Session Manager** (gak butuh port SSH publik lagi) |
| RAM safety net | Tidak direncanakan | Swap 2GB (wajib, bukan opsional) — kejadian lagi di EC2 |
| CI image arch | Tidak dipikirin (asumsi `amd64` di mana-mana) | Multi-arch → **`arm64`-only native runner** setelah Nusa (x86) pensiun |
| Observability | Wiring kode doang | Grafana Cloud + Alloy, beneran kekirim — jalan sama persis di EC2 |
| Uptime monitoring | Tidak direncanakan | UptimeRobot (ping `/health` tiap 5 menit) |
