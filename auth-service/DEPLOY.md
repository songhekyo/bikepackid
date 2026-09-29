# Deploy ke VPS Nusa — historis, superseded

**Sudah tidak dipakai** sejak cutover ke AWS EC2, 28 September 2026 — panduan operasional aktif sekarang ada di [`DEPLOY_AWS.md`](./DEPLOY_AWS.md). File ini dibiarin ada sebagai referensi (banyak prinsipnya, kayak Watchtower/Grafana Alloy/nginx reverse-proxy, tetep sama persis di setup baru) dan buat jejak keputusan — lihat `docs/INFRA_HISTORY.md` kenapa pindah.

Panduan ini sengaja manual — tujuannya bukan cuma "biar jalan", tapi biar kamu pegang langsung tiap lapisan: server Linux, firewall, Docker, reverse proxy, TLS, database managed. Jalankan tiap perintah di bawah satu-satu, jangan asal copy-paste semuanya sekaligus.

Dulu live di `https://bikepacking.cyou`, di-deploy ke VPS Nusa (1 vCPU / 1GB RAM / 25GB disk, Fedora 42, Jakarta). Panduan ini udah direvisi total berdasarkan apa yang **beneran kejadian & kepake** waktu deploy pertama kali, bukan rencana awal di atas kertas. Bedanya:

- **Provider & spek**: awalnya rencana Hetzner CX22 (2 vCPU/4GB RAM), yang kepake justru VPS 1GB RAM — jauh lebih ketat, makanya ada langkah **swap** yang gak ada di draft awal (tanpa itu, `docker build` bisa OOM-killed).
- **Reverse proxy**: awalnya rencana Caddy (auto-HTTPS), yang kepake **nginx** + `certbot` manual.
- **Database**: awalnya rencana Postgres jalan sebagai container di VPS yang sama, yang kepake **Supabase** (managed Postgres) — sekalian ngirit RAM di VPS yang cuma 1GB.
- **Firewall**: image Fedora dari provider ini **tidak** bawa `firewalld` ter-install (beda dari asumsi awal "biasanya udah ada default").
- **Observability**: awalnya cuma "wiring kode, belum ada tujuan", sekarang beneran nyala — trace kekirim ke Grafana Cloud lewat Grafana Alloy (Fase 7).

## Fase 0 — Siapkan SSH key (di laptop kamu, bukan di server)

```bash
ssh-keygen -t ed25519 -C "bikepackid-vps"
```

Ini bikin sepasang kunci: privat (`~/.ssh/id_ed25519`, jangan pernah dikirim ke mana pun) dan publik (`~/.ssh/id_ed25519.pub`, ini yang didaftarkan ke server). Nanti pas login SSH, server verifikasi kamu punya kunci privat yang cocok — lebih aman daripada password.

Kalau mau akses dari device lain juga (HP, dll), generate keypair **baru** khusus device itu (jangan copy private key yang sama ke device lain), lalu tambahkan public key-nya ke `~/.ssh/authorized_keys` di server pakai `>>` (append, bukan overwrite):
```bash
echo "PUBLIC_KEY_DEVICE_BARU" >> ~/.ssh/authorized_keys
```
Tiap baris di `authorized_keys` itu satu device yang diizinkan — kalau device hilang, cabut aksesnya dengan hapus baris itu doang, device lain tetap jalan.

## Fase 1 — Provision server

Spek yang kepake: **1 vCPU / 1GB RAM / 25GB disk, Fedora 42**. Ini di ujung bawah yang masih workable — cukup buat jalanin backend (database-nya di luar, lihat Fase 3), tapi ketat pas fase build (lihat catatan swap di Fase 2). Kalau providermu nawarin RAM lebih (2GB+), itu ngasih lebih banyak headroom, tapi bukan keharusan — panduan ini udah dites jalan di 1GB.

Daftarkan public key `~/.ssh/id_ed25519.pub` ke server pas provisioning (tiap provider beda UI-nya, cari opsi "SSH Key" pas bikin server baru).

## Fase 2 — Masuk & setup dasar server

```bash
ssh root@<IP_SERVER>
```
(Kalau `root@` ditolak, coba `ssh fedora@<IP_SERVER>` dengan `sudo` di depan tiap perintah root di bawah.)

Update sistem:
```bash
dnf upgrade --refresh -y
```
Biarin sampai selesai penuh — jangan di-`Ctrl+C`, ini bisa ninggalin sistem dalam kondisi paket setengah ke-upgrade.

**Swap space** — wajib di server RAM kecil kayak ini, bukan opsional. `cargo build --release` (bagian dari `docker build` nanti) bisa butuh RAM lebih dari 1GB sesaat waktu compile/link; tanpa swap, itu bisa ke-`OOM kill` di tengah build (proses mati paksa sama kernel, gagal total, harus ulang dari awal). Swap kasih "RAM cadangan" pakai disk — lebih lambat dari RAM asli, tapi build tetap **selesai** daripada mati:
```bash
fallocate -l 2G /swapfile
chmod 600 /swapfile
mkswap /swapfile
swapon /swapfile
echo '/swapfile none swap sw 0 0' | tee -a /etc/fstab
free -h
```
Baris terakhir (`tee -a /etc/fstab`) penting — tanpa itu, swap aktif sekarang tapi hilang lagi kalau server reboot.

**Firewall** — cek dulu, jangan asumsi udah ada:
```bash
which firewall-cmd
```
Kalau kosong (belum ke-install), pasang dulu:
```bash
dnf install -y firewalld
systemctl enable --now firewalld
```
Baru buka port yang perlu (prinsipnya: tolak semua default, buka cuma yang benar-benar perlu):
```bash
firewall-cmd --add-service=ssh --permanent
firewall-cmd --add-port=8080/tcp --permanent
firewall-cmd --reload
firewall-cmd --list-all
```
(Port `8080` ini sementara, buat testing curl langsung di Fase 4 — nanti ditutup lagi begitu nginx jadi satu-satunya pintu masuk, lihat Fase 5.)

**Install Docker:**
```bash
curl -fsSL https://get.docker.com | sh
systemctl enable --now docker
```

## Fase 3 — Siapkan database (Supabase)

Database **tidak** dijalankan sebagai container lokal di VPS ini — pakai [Supabase](https://supabase.com) (managed Postgres), sekalian ngirit RAM.

1. Bikin project baru di Supabase.
2. **Project Settings → Database → Connection string**, pilih tab **Connection pooling**, metode **Session pooler** (bukan "Direct connection", bukan "Transaction pooler"):
   - Direct connection defaultnya **IPv6-only** — container Docker biasanya cuma punya akses IPv4 di network default-nya (walau host-nya sendiri punya IPv6), jadi direct connection gagal dengan error `Network is unreachable`.
   - Transaction pooler cocok buat aplikasi stateless/serverless (koneksi singkat-singkat) — bukan pola kita, karena backend bikin connection pool sendiri yang persisten (`sqlx::PgPoolOptions`).
   - **Session pooler** itu alternatif IPv4 buat pola koneksi persisten kayak punya kita — inilah yang dipakai.
3. Copy connection string-nya (format URI), simpan buat `.env` di Fase 4. **Jangan** paste password database ini ke tempat lain (chat, issue tracker, dst) — kalau ke-paste di luar `.env`, anggap ke-expose, rotate password-nya (Project Settings → Database → Reset database password).

## Fase 4 — Deploy aplikasinya

```bash
dnf install -y git
git clone -b claude/bikepacker-portal-platform-i30wvy https://github.com/songhekyo/bikepackid.git
cd bikepackid/backend
```
(Sesuaikan branch kalau PR yang nambahin file-file deploy ini udah di-merge ke `main` — tinggal `git clone` biasa tanpa `-b`.)

Siapkan `.env` dari template:
```bash
cp .env.production.example .env
nano .env
```
Isi tiap placeholder:
- `DATABASE_URL` — connection string Session pooler dari Fase 3.
- `JWT_SECRET` — generate random: `openssl rand -base64 48`.
- `GOOGLE_CLIENT_ID` / `GOOGLE_CLIENT_SECRET` — dari Google Cloud Console (lihat catatan OAuth di bawah).
- `GOOGLE_REDIRECT_URL` — `https://domain-kamu/auth/google/callback` (**bukan** `/api/auth/callback/google` — itu konvensi NextAuth.js, beda framework; route yang beneran ada di backend ini cuma `/auth/google/callback`).
- `FRONTEND_URL` — `https://domain-kamu`.
- `COOKIE_SECURE` — hapus baris ini (default `true`, pas buat HTTPS yang udah kita pasang dari awal berkat nginx+certbot di Fase 5).

**Update (lihat `docs/INFRA_HISTORY.md`): server ini gak build Rust lagi.** Awalnya build image langsung di VPS (15–40 menit di CPU sekecil ini, perlu `tmux` supaya gak ikut mati kalau SSH putus — BuildKit nge-cancel build begitu client-nya putus). Sekarang `.github/workflows/ci.yml` yang build image di GitHub Actions dan push ke GHCR (`ghcr.io/songhekyo/bikepackid-backend`) setiap push ke `main`, dan **Watchtower** (service di `docker-compose.yml`) yang polling GHCR tiap 5 menit dan auto-`pull`+restart `backend` begitu ada image baru — jadi normalnya kamu **gak perlu deploy manual sama sekali** setelah setup awal ini. Package GHCR-nya public, jadi gak perlu `docker login`.

Setup pertama kali (sekali aja — abis ini Watchtower yang jalanin update-nya):
```bash
docker compose up -d
```

Cek jalan atau tidak:
```bash
docker compose ps
docker compose logs -f backend
docker compose logs -f watchtower
```
Migrasi database jalan otomatis saat backend start — cari baris `bikepackid backend listening on port 8080` di log, tanpa `panicked at ...` sebelumnya. `alloy` sengaja **gak** ikut di-auto-update Watchtower (lihat komentar di `docker-compose.yml`) — upgrade Alloy tetep manual/`docker compose pull alloy && docker compose up -d alloy`, karena config syntax-nya pernah berubah antar versi.

Cek **commit mana** yang lagi live (gak perlu inspect digest image manual):
```bash
curl https://domain-kamu/version
```
Balasnya `{"git_sha": "<commit sha>"}` — commit SHA itu ke-bake ke image waktu CI build (lihat `.github/workflows/ci.yml`), bukan dibaca dari `.env`, jadi selalu akurat sama image yang beneran jalan. Kalau abis merge PR nilainya belum berubah, tunggu sampai ~5 menit (interval polling Watchtower) sebelum curiga ada yang salah.

## Fase 5 — Domain asli + HTTPS via nginx

1. Di DNS provider domain kamu, bikin **A record** menunjuk ke IP server:
   ```bash
   dig domain-kamu +short   # verifikasi udah resolve ke IP server
   ```
2. Install nginx:
   ```bash
   dnf install -y nginx
   systemctl enable --now nginx
   ```
3. Matikan default server block bawaan (`/etc/nginx/nginx.conf`, cari blok `server { listen 80; ... }`, comment-out atau hapus) — soalnya bakal bentrok sama config reverse-proxy yang kita bikin, sama-sama `listen 80`.
4. Bikin config reverse-proxy:
   ```bash
   tee /etc/nginx/conf.d/bikepackid.conf << 'EOF'
   server {
       listen 80 default_server;
       listen [::]:80 default_server;
       server_name domain-kamu;

       location / {
           proxy_pass http://127.0.0.1:8080;
           proxy_http_version 1.1;
           proxy_set_header Connection "";

           proxy_set_header Host $host;
           proxy_set_header X-Real-IP $remote_addr;
           proxy_set_header X-Forwarded-For $remote_addr;
           proxy_set_header X-Forwarded-Proto $scheme;
       }
   }
   EOF
   ```
   `X-Forwarded-For $remote_addr` (bukan `$proxy_add_x_forwarded_for`) sengaja — backend pakai header ini buat rate limiting per-IP (`tower_governor`), dan `$remote_addr` meng-**overwrite** total (bukan nambahin ke) apa pun yang klien coba kirim, supaya rate limiter gak bisa dibohongi lewat header palsu dari luar.
5. **SELinux** (Fedora aktifkan *enforcing* default) — tanpa ini, nginx kena `502 Bad Gateway` walau config di atas udah benar, karena SELinux nolak nginx bikin koneksi keluar ke port lain:
   ```bash
   setsebool -P httpd_can_network_connect on
   ```
6. Test & reload:
   ```bash
   nginx -t
   systemctl reload nginx
   ```
7. Buka port HTTP di firewall:
   ```bash
   firewall-cmd --add-service=http --permanent
   firewall-cmd --reload
   ```
8. Install `certbot`, minta sertifikat TLS (otomatis edit config nginx buat block HTTPS-nya juga):
   ```bash
   dnf install -y certbot python3-certbot-nginx
   certbot --nginx -d domain-kamu
   ```
   Ikutin prompt-nya (email buat notifikasi expiry, pilih redirect HTTP→HTTPS otomatis).
9. Buka port HTTPS, tutup akses langsung ke `8080` dari luar (backend cuma boleh diakses lewat nginx sekarang):
   ```bash
   firewall-cmd --add-service=https --permanent
   firewall-cmd --remove-port=8080/tcp --permanent
   firewall-cmd --reload
   ```

## Fase 6 — Setup Google OAuth Console

Di [Google Cloud Console](https://console.cloud.google.com/apis/credentials), OAuth Client ID kamu:
- **Authorized redirect URIs**: `https://domain-kamu/auth/google/callback` — harus **exact match** sama `GOOGLE_REDIRECT_URL` di `.env`.
- **Authorized JavaScript origins**: `https://domain-kamu` — gak krusial buat flow kita (itu buat OAuth client-side/JS, flow kita full server-side redirect), tapi gak masalah diisi.

Setelah `.env` diupdate (`GOOGLE_REDIRECT_URL`, `FRONTEND_URL` pakai domain HTTPS) dan container di-restart (`docker compose up -d`), tes lengkap:
```bash
curl https://domain-kamu/health
curl -i https://domain-kamu/auth/google/login
```
`/health` balas `ok`, `/auth/google/login` balas `303` redirect ke `accounts.google.com`. Buat verifikasi login beneran, buka `/auth/google/login` di **browser** (bukan `curl` — perlu login interaktif) dan pantau `docker compose logs -f backend` bareng waktu — baris `GET /auth/google/callback ... status_code=303` artinya kode berhasil ditukar, user ke-upsert, session ke-buat.

## Fase 7 — Trace ke Grafana Cloud (opsional, tapi disarankan)

Backend udah punya instrumentasi OpenTelemetry bawaan (`telemetry.rs`), tinggal kasih tempat tujuan. Kita pakai [Grafana Cloud](https://grafana.com) (ada free tier) + **Grafana Alloy** sebagai collector lokal — backend ngirim ke Alloy tanpa autentikasi (satu jaringan Docker), Alloy yang pegang kredensial buat forward ke Grafana Cloud.

1. Daftar Grafana Cloud, pilih **"Set up app monitoring"** pas onboarding (bukan "Monitor infrastructure"/"Monitor website uptime" — itu produk beda).
2. Di **Grafana Cloud Portal** (`grafana.com`, bukan instance Grafana-nya) → stack kamu → **Details** → cari card **OpenTelemetry** → **Configure**. Catat 3 nilai: **OTLP Endpoint**, **Instance ID**, generate **API Token** (kepencet sekali doang, langsung simpan).
3. Tambahin ke `.env` server:
   ```
   OTEL_EXPORTER_OTLP_ENDPOINT=http://alloy:4318
   GRAFANA_CLOUD_OTLP_ENDPOINT=<OTLP Endpoint dari langkah 2>
   GRAFANA_CLOUD_INSTANCE_ID=<Instance ID dari langkah 2>
   GRAFANA_CLOUD_API_KEY=<API Token dari langkah 2>
   ```
   (`OTEL_EXPORTER_OTLP_ENDPOINT` beda dari `GRAFANA_CLOUD_OTLP_ENDPOINT` — yang pertama nunjuk ke Alloy lokal, yang kedua dipakai Alloy buat forward ke Grafana Cloud beneran. Jangan ketuker.)
4. `deploy/config.alloy` dan service `alloy` di `docker-compose.yml` udah ada di repo, gak perlu bikin manual. Jalanin:
   ```bash
   docker compose up -d
   docker compose logs -f alloy
   ```
   Pastikan Alloy start bersih (gak ada `Error:` di log).
5. Generate trafik (`curl https://domain-kamu/health` beberapa kali), tunggu ~10 detik, cek di Grafana Cloud: **Explore** → pilih datasource yang namanya ada **`-traces`** (bukan `-prom`, itu buat metrics) → tab **TraceQL** → ketik `{}` → **Shift+Enter**. Trace `bikepackid_backend` harusnya muncul.

## Perintah yang sering kepake

| Perintah | Fungsi |
|---|---|
| `docker compose ps` | status container |
| `docker compose logs -f backend` | log realtime backend |
| `docker compose logs -f alloy` | log realtime Alloy (collector trace) |
| `docker compose logs -f watchtower` | log realtime Watchtower (auto-deploy backend) |
| `docker compose restart backend` | restart tanpa pull ulang (**tidak** baca ulang `.env`) |
| `docker compose up -d` | recreate container kalau `.env` berubah (baca ulang env) |
| `docker compose pull backend && docker compose up -d backend` | paksa deploy manual sekarang juga, gak nunggu Watchtower (5 menit) |
| `docker compose down` | matikan backend (database ada di luar — Supabase, tidak kepengaruh) |
| `df -h` / `free -h` | cek sisa disk / RAM+swap |
| `firewall-cmd --list-all` | cek aturan firewall aktif |
| `nginx -t` | cek syntax config nginx sebelum reload |
| `tail -f /var/log/nginx/access.log` / `error.log` | log nginx realtime |
| `sudo journalctl -u docker` | log Docker Engine (kalau `docker compose` aneh) |
