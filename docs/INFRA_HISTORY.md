# Riwayat infra — dari kosong sampai live

Catatan kronologis kenapa infra `bikepackid` bentuknya kayak sekarang. Bukan panduan langkah-demi-langkah (itu di [`backend/DEPLOY.md`](../backend/DEPLOY.md)) — ini dokumentasi **keputusan dan perubahan**, termasuk yang rencana awalnya beda dari yang akhirnya kepake, dan kenapa.

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

## 9. Verifikasi live

Setelah semua di atas jalan, dites dari luar server (laptop, bukan `curl` di server sendiri):
```
GET /auth/google/login  → 303 ke accounts.google.com (PKCE + state ada)
GET /auth/google/callback → 303 (redirect sukses, latency ~2s buat tukar code + fetch profil + upsert user)
```
Dikonfirmasi lewat `docker compose logs -f backend` (structured JSON log, `request_id` per request) berbarengan sama tes login manual di browser.

## Ringkasan: rencana vs. kenyataan

| Komponen | Rencana awal | Yang kepake |
|---|---|---|
| Provider VPS | Hetzner CX22 (4GB RAM) | Nusa (1GB RAM) |
| Reverse proxy | Caddy (auto-TLS) | nginx + certbot |
| Database | Postgres lokal (container) | Supabase (Session pooler) |
| Firewall | Asumsi udah ada | `firewalld` diinstall manual |
| RAM safety net | Tidak direncanakan | Swap 2GB (wajib, bukan opsional) |
