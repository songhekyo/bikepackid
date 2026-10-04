# Deploy ke AWS EC2 — panduan yang live sekarang

Menggantikan [`DEPLOY.md`](./DEPLOY.md) (VPS Nusa/Fedora) sebagai panduan operasional aktif, sejak cutover 28 September 2026. `DEPLOY.md` dibiarin ada sebagai referensi historis — banyak prinsipnya (Watchtower, Grafana Alloy, nginx reverse-proxy) tetep sama persis, cuma OS-level command yang beda. Rasional lengkap kenapa pindah ke AWS, kenapa pilih Graviton/`arm64`, dan kenapa Postgres balik self-hosted (bukan Supabase) ada di [`docs/AWS_MIGRATION.md`](../docs/AWS_MIGRATION.md); bug-bug yang ketemu pas eksekusi ada di [`docs/INFRA_HISTORY.md`](../docs/INFRA_HISTORY.md) bagian 12.

Live di `https://bikepacking.cyou`, EC2 `t4g.small` (2 vCPU/2GB RAM, ARM Graviton, Ubuntu 26.04), region `ap-southeast-2` Sydney.

## Akses instance: SSM Session Manager, bukan SSH biasa

IP publik laptop kamu kemungkinan berubah-ubah (ISP dynamic, ganti jaringan) — daripada terus update rule "My IP" di security group, akses instance lewat **AWS Console → EC2 → Instances → pilih instance → Connect → tab Session Manager**. Ini gak butuh key/IP whitelist sama sekali (otentikasi IAM). Port 22 di security group udah dicabut.

Begitu masuk, switch ke user `ubuntu` (session default-nya `ssm-user`):
```bash
sudo su - ubuntu
cd ~/bikepackid/auth-service
```

## Setup awal (sekali doang, pas provisioning instance baru)

Provisioning EC2 (VPC, Security Group, instance type, Elastic IP) — lihat `docs/AWS_MIGRATION.md` Fase 1. Setelah instance `Running`:

```bash
sudo apt update && sudo apt full-upgrade -y

# Swap wajib — instance kecil (t4g.small = 2GB) jalanin auth-service +
# journey-service + Postgres bareng, RAM ketat sama kayak alasan swap di
# VPS lama.
sudo fallocate -l 2G /swapfile
sudo chmod 600 /swapfile
sudo mkswap /swapfile
sudo swapon /swapfile
echo '/swapfile none swap sw 0 0' | sudo tee -a /etc/fstab

# Docker — script resmi, sama kayak di VPS lama
curl -fsSL https://get.docker.com | sh
sudo systemctl enable --now docker
sudo usermod -aG docker ubuntu   # perlu re-login biar kepake tanpa sudo

sudo apt install -y git nginx certbot python3-certbot-nginx

git clone https://github.com/songhekyo/bikepackid.git
cd bikepackid/auth-service
```

**Firewall level-OS**: sengaja **di-skip** — Security Group AWS udah jadi firewall di level hypervisor (di luar jangkauan instance sendiri), beda dari VPS lama yang butuh `firewalld` manual karena gak punya proteksi setara.

## Database: Postgres self-hosted (bukan Supabase)

Beda dari VPS lama, instance ini jalanin Postgres-nya sendiri sebagai container — `docker-compose.postgres.yml` adalah **override** yang di-merge di atas `docker-compose.yml` dasar, khusus environment yang gak pakai Supabase:

```bash
docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d
```

(Semua command `docker compose` di bawah pakai kombinasi 2 file ini — biasakan alias kalau males ngetik panjang: `alias dc='docker compose -f docker-compose.yml -f docker-compose.postgres.yml'`)

`.env` butuh 2 variabel tambahan yang gak ada di `.env.production.example` (yang nulis skenario Supabase):
```
POSTGRES_PASSWORD=<generate: openssl rand -hex 24>
DATABASE_URL=postgres://bikepackid:<POSTGRES_PASSWORD-yang-sama>@postgres:5432/bikepackid
```
**Penting**: pakai `openssl rand -hex`, bukan `-base64` — base64 bisa hasilin `+`/`/`/`=` yang bikin `DATABASE_URL` gagal di-parse sebagai URL (`InvalidPort`).

Sisa variabel `.env` lainnya (JWT_SECRET, GOOGLE_*, R2_*, GRAFANA_CLOUD_*) ikutin `.env.production.example` — kalau ini instance pengganti (bukan yang pertama kali), **`JWT_SECRET` harus sama persis** kayak yang lama, biar session/cookie user yang udah login gak invalid begitu cutover.

## Deploy pertama kali & seterusnya

```bash
docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d
docker compose -f docker-compose.yml -f docker-compose.postgres.yml logs -f auth-service
```
Cari `bikepackid auth-service listening on port 8080` tanpa `panicked at ...`. Setelah ini, **Watchtower yang jalanin update-nya otomatis** — sama persis kayak VPS lama, gak ada bedanya (Watchtower gak peduli host-nya di mana).

**Jangan cuma percaya `docker ps` buat mastiin Watchtower beneran jalan** — "Up" cuma berarti container-nya nyala, bukan berarti update-nya berhasil. Watchtower pernah diam-diam gagal total berminggu-minggu (401 ke GHCR, `docker-compose.yml` sekarang udah fix mount `config.json`-nya) tanpa ada tanda apapun selain `docker logs backend-watchtower-1` dan `curl localhost:8080/version` ketinggalan jauh dari `main` — lihat `docs/INFRA_HISTORY.md` bagian 13. Cek berkala kalau ragu:
```bash
curl -s http://localhost:8080/version   # bandingin sama `git log origin/main -1`
docker logs backend-watchtower-1 --since 24h | grep -i unauthorized
```

**Image harus `arm64`** — `t4g.small` itu Graviton/ARM, CI (`.github/workflows/ci.yml`) build khusus `arm64` (native runner `ubuntu-24.04-arm`, bukan `amd64`+emulasi). Kalau ketemu `exec format error`, itu tanda ada yang salah build target arch-nya, bukan masalah di instance ini.

## Reverse proxy + HTTPS

Sama persis kayak VPS lama (nginx + certbot), cuma `sites-available`/`sites-enabled` (Ubuntu) bukan single-file `nginx.conf` (Fedora):

```bash
sudo rm /etc/nginx/sites-enabled/default   # matiin default server block bawaan
sudo tee /etc/nginx/sites-available/bikepackid.conf << 'EOF'
server {
    listen 80 default_server;
    listen [::]:80 default_server;
    server_name bikepacking.cyou;

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
sudo ln -s /etc/nginx/sites-available/bikepackid.conf /etc/nginx/sites-enabled/
sudo nginx -t && sudo systemctl reload nginx

sudo certbot --nginx -d bikepacking.cyou
```
**Gak ada langkah SELinux** — Ubuntu gak aktifin itu default (beda dari Fedora yang butuh `setsebool -P httpd_can_network_connect on`).

## Rename `backend` → `auth-service` (cutover sekali doang)

Directory dan crate ini dulu namanya `backend` — di-rename ke `auth-service` begitu isinya cuma tersisa auth/session/user setelah Journey/Checkpoint/Post pindah ke `journey-service`. Ini termasuk image GHCR: `ghcr.io/songhekyo/bikepackid-backend` → `ghcr.io/songhekyo/bikepackid-auth-service`.

**Konsekuensi buat instance yang udah live**: begitu PR rename ini merge ke `main`, CI berhenti nge-push ke tag `bikepackid-backend` lama (Watchtower di instance masih nge-track tag lama, jadi container yang jalan sekarang **gak error**, cuma berhenti dapet update otomatis). Perlu satu langkah manual di instance buat pindah ke tag baru:
```bash
cd ~/bikepackid
git pull
cd auth-service   # direktori lokal juga ikut ke-rename setelah git pull
docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d
docker compose -f docker-compose.yml -f docker-compose.postgres.yml logs -f auth-service
```
Cari `bikepackid auth-service listening on port 8080` tanpa `panicked at ...`. Setelah ini, Watchtower otomatis nge-track tag baru (`docker-compose.yml` yang di-`git pull` udah nunjuk ke situ) — gak perlu ngapa-ngapain lagi abis ini.

**Gak ada perubahan env var** — `JWT_SECRET`/`DATABASE_URL`/dst tetep sama persis, cuma nama service & image-nya yang beda. Session/cookie user yang udah login **tidak** invalid — JWT-nya gak berubah format, cuma proses yang ngeluarin/verifikasi dia yang ganti nama.

**Volume Postgres aman** — `docker-compose.yml` sekarang pin `name: backend` di level top (Compose project name), sengaja **tetap** `backend` walau direktorinya udah `auth-service` — supaya `docker compose up -d` abis rename ini tetep nempel ke volume `backend_postgres_data` yang udah ada datanya, bukan bikin volume kosong baru gara-gara nama project ke-derive dari nama direktori yang berubah. Gak perlu ngapa-ngapain soal ini, cuma dicatat di sini biar jelas kenapa `name: backend` ada padahal direktorinya `auth-service`.

## journey-service (Journey/Checkpoint/Post, Fase 2)

Kedua image (`bikepackid-auth-service` dan `bikepackid-journey-service`) di-build+push otomatis tiap push ke `main` (lihat `.github/workflows/ci.yml`), tapi **belum otomatis jalan** di instance manapun sampai `docker-compose.yml` di server benar-benar mendefinisikan servicenya — `journey-service` udah ditambahin ke `docker-compose.yml` di repo ini, tapi deploy pertama kalinya tetap manual sekali:

```bash
docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d
docker compose -f docker-compose.yml -f docker-compose.postgres.yml logs -f journey-service
```
Cari `bikepackid journey-service listening on port 8081` tanpa `panicked at ...`. Setelah ini, Watchtower ikut nge-track service ini juga (label `watchtower.enable=true` udah ada di compose-nya) — sama kayak auth-service, gak perlu deploy manual lagi abis ini.

**Gak ada env var baru yang perlu diisi** — `journey-service` pakai `.env` yang sama persis dengan auth-service (`DATABASE_URL`, `JWT_SECRET` — *harus* sama biar sesi login yang auth-service keluarin valid buat journey-service juga, `FRONTEND_URL`, `R2_*`, `OTEL_EXPORTER_OTLP_ENDPOINT`); `PORT`-nya di-override ke `8081` langsung di `docker-compose.yml`, bukan dari `.env`.

**Nginx routing** — ini yang masih manual, belum ke-otomasi. Endpoint-endpoint journey/checkpoint/post (`/journeys`, `/me/journeys`, `/checkpoints/*`, `/uploads/presign-url`) perlu di-route ke `127.0.0.1:8081`, sisanya tetep ke `127.0.0.1:8080`. Tambahin `location` block sebelum `location /` di `/etc/nginx/sites-available/bikepackid.conf`:
```nginx
    location ~ ^/(journeys|me/journeys|checkpoints|uploads/presign-url) {
        proxy_pass http://127.0.0.1:8081;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
    }
```
lalu `sudo nginx -t && sudo systemctl reload nginx`. Sebelum ini di-apply, frontend yang manggil endpoint-endpoint itu masih ngarah ke backend lama (yang udah gak punya route-nya lagi setelah PR ekstraksi journey-service) — jangan reload nginx dengan block ini sebelum bener-bener siap cutover trafik-nya.

**Catatan basi**: regex di atas belum nyakup endpoint yang ditambahin belakangan (`equipment-categories`, `/journeys/:id/equipment`, `/journeys/:id/sponsors`) — masih ke-proxy ke `location /` (auth-service, 8080) yang salah. Belum di-fix di sini karena di luar scope task yang lagi dikerjain pas ini ditulis; perlu diinget kalau endpoint equipment/sponsor mulai dipanggil dari frontend.

## Landing page statis (`web/`) — riwayat, sekarang di-serve dari Cloudflare

Halaman "coming soon" (`web/index.html`) awalnya di-serve langsung oleh nginx EC2 dari hasil `git pull` di `bikepacking.cyou`. Itu udah **digantikan** oleh setup Cloudflare Workers di bagian bawah — `location = /` yang nyajiin file ini masih ada di config nginx EC2 tapi **gak kepake lagi** (server block `bikepacking.cyou` sekarang 301 redirect duluan, sebelum sempat nyentuh `location` manapun). Dibiarin aja, gak ganggu, tinggal dibersihin kapan-kapan kalau sempat.

Isi halamannya sendiri udah berubah dua kali sejak pertama dibikin: brand "bikepacking.id" (mascot frog, favicon file `frog.svg`) → rebrand ke "Taktik dan Siasat" (mascot heart) → favicon jadi inline `data:image/svg+xml` di `<head>`-nya sendiri, gak ada file terpisah lagi. Riwayat lengkapnya ada di commit history `web/index.html`.

**Tombol "Masuk dengan Google"** ngarah ke `/auth/google/login` (rute asli di `auth-service`, bukan `/auth/google`) — ini bug yang muncul berkali-kali tiap ada versi baru halaman diupload, selalu dicek-ulang sebelum di-commit.

**Belum ada**: endpoint `/api/seats` dan `/api/waitlist` yang dipanggil script di halaman ini — `/api/seats` gagal dengan aman (fallback ke angka statis), tapi form waitlist bakal 404 beneran kalau disubmit. Belum dibangun.

## Migrasi ke `taktikdansiasat.com` (Cloudflare Workers)

Rebrand dari "bikepacking.id" ke "Taktik dan Siasat" diikuti pindah domain produksi juga, dari `bikepacking.cyou` (A record langsung ke EC2, nginx+Certbot biasa) ke `taktikdansiasat.com` yang di-serve lewat **Cloudflare Workers** (bukan nginx EC2 langsung) buat halaman statis, dengan EC2 tetap jadi origin API lewat hostname terpisah.

### Arsitektur

```
Browser → taktikdansiasat.com (Cloudflare, proxied)
            │
            ├─ GET/HEAD cocok file statis (web/index.html) → served dari Cloudflare, gak nyentuh EC2 sama sekali
            │
            └─ selain itu (semua API path + semua method non-GET/HEAD)
                  → Worker fetch() ke origin.taktikdansiasat.com (EC2, DNS-only/gak di-proxy Cloudflare)
                        → nginx EC2 (location regex yang sama kayak sebelumnya) → auth-service (8080) / journey-service (8081)

bikepacking.cyou (domain lama) → 301 redirect ke taktikdansiasat.com, gak serve apa-apa sendiri lagi
```

### File-file di repo

- `wrangler.jsonc` (root repo) — config Cloudflare Workers: `assets.directory` nunjuk ke `web/`, `main` nunjuk ke `worker/index.js`, `assets.binding: "ASSETS"` (wajib ada begitu ada `main` script, kalau gak Worker-nya gak bisa akses file statisnya sama sekali).
- `worker/index.js` — fetch handler: GET/HEAD coba serve asset statis dulu, kalau 404 (atau method-nya bukan GET/HEAD) di-forward ke `https://origin.taktikdansiasat.com`. **Sengaja gak ada daftar prefix path API** (`/auth/*`, `/journeys/*`, dst) yang di-hardcode — regex nginx di EC2 udah kebukti gampang basi (lihat catatan di bagian journey-service), jadi desainnya "coba statis dulu, sisanya lempar ke origin" biar gak ada daftar yang perlu diinget-inget diupdate tiap nambah endpoint baru.
- `package.json` (root) — minimal, cuma declare `wrangler` sebagai devDependency biar `npx wrangler` di CI Cloudflare resolve bersih.

### Deploy

Dashboard Cloudflare **Workers & Pages** → project **bikepackingid** (nama project beda dari `name` di `wrangler.jsonc` — Cloudflare override pakai nama project pas pertama dibikin, cuma warning, gak masalah fungsional) di-connect ke repo GitHub ini via "Connect to Git" (OAuth GitHub App, bukan "Clone via Git URL" yang cuma sekali doang gak auto-deploy). Tiap push ke `main` otomatis trigger build+deploy baru — gak ada langkah manual di server EC2 buat update halaman statisnya.

Settings → Builds punya **dua** command terpisah yang gampang ketuker: tab **Production** (`main`) pake field "Deploy command", tab **Previews Base** (branch lain/PR) pake field "Preview command" — isinya harus sama-sama `npx wrangler deploy`, bukan `npx wrangler versions upload` (itu cuma upload versi, gak nge-live-in apa-apa sampai di-promote manual) atau `npx wrangler preview` (udah deprecated). Kalau salah satu field ini diubah lewat dashboard, **"Retry build" di build yang udah gagal gak kebaca perubahannya** — Cloudflare snapshot command-nya pas build itu pertama kali di-trigger, bukan baca ulang config terbaru. Harus push commit baru (bukan retry) biar build berikutnya beneran pake command yang udah diupdate.

### Setup DNS (Cloudflare)

1. Domain `taktikdansiasat.com` di-"Connect domain" di Cloudflare (bukan "Transfer" — kepemilikan/registrasi tetap di Hostinger), nameserver registrar (Hostinger) diganti ke 2 nameserver yang dikasih Cloudflare.
2. Record `A` default hasil scan Cloudflare (nunjuk ke IP parking Hostinger) **dihapus** — itu bukan EC2, cuma placeholder.
3. Custom domain `taktikdansiasat.com` di-attach ke Worker-nya (project → tab Domains → Add → kosongin field subdomain buat root domain).
4. Record tambahan buat origin: `A` `origin` → IP Elastic EC2, proxy status **DNS only** (awan abu-abu) — wajib DNS-only, kalau di-proxy Cloudflare, `fetch()` dari dalam Worker ke hostname ini bakal infinite-loop balik ke Cloudflare lagi alih-alih nyampe ke EC2.

### Setup EC2 (origin terpisah buat API)

`origin.taktikdansiasat.com` butuh cert TLS sendiri karena dia server block terpisah dari `bikepacking.cyou`/`taktikdansiasat.com` (nginx cuma bisa nyajiin satu cert per server block). **Jangan** pakai `certbot --expand` kalau tujuannya nambahin domain ke server block yang beda dari yang lagi dia proses — kejadian nyata di migrasi ini, `--expand` malah bikin cert BARU terpisah terus nimpa `ssl_certificate` path di server block yang di-share, bikin `bikepacking.cyou` sempat down (cert mismatch, `NET::ERR_CERT_COMMON_NAME_INVALID`) sampai dipisah jadi dua server block masing-masing cert sendiri:

```nginx
server {
    server_name origin.taktikdansiasat.com;

    location ~ ^/(journeys|me/journeys|checkpoints|uploads/presign-url) {
        proxy_pass http://127.0.0.1:8081;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    location / {
        proxy_pass http://127.0.0.1:8080;
        proxy_http_version 1.1;
        proxy_set_header Connection "";
        proxy_set_header Host $host;
        proxy_set_header X-Real-IP $remote_addr;
        proxy_set_header X-Forwarded-For $remote_addr;
        proxy_set_header X-Forwarded-Proto $scheme;
    }

    listen [::]:443 ssl;
    listen 443 ssl;
    ssl_certificate /etc/letsencrypt/live/origin.taktikdansiasat.com/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/origin.taktikdansiasat.com/privkey.pem;
    include /etc/letsencrypt/options-ssl-nginx.conf;
    ssl_dhparam /etc/letsencrypt/ssl-dhparams.pem;
}
```

Cert-nya sendiri didapat dengan (cert baru, bukan expand):
```bash
sudo certbot --nginx -d origin.taktikdansiasat.com
```

Server block `bikepacking.cyou` sekarang jadi redirect doang (lihat bagian "cutover" di bawah), dan server block port-80 (`return 404`, `# managed by Certbot`) buat `bikepacking.cyou` **gak disentuh** sepanjang migrasi ini.

### OAuth

Domain berubah beneran (bukan cuma rename direktori kayak migrasi `backend`→`auth-service` dulu yang domain-nya tetap) — jadi **redirect URI wajib diupdate**, beda dari migrasi-migrasi sebelumnya:
1. Google Cloud Console → Credentials → OAuth client → Authorized redirect URIs → tambah `https://taktikdansiasat.com/auth/google/callback` (URI lama buat `bikepacking.cyou` dibiarin, gak dihapus, buat jaga-jaga selama transisi).
2. `.env` di EC2: `GOOGLE_REDIRECT_URL=https://taktikdansiasat.com/auth/google/callback`.
3. `docker compose -f docker-compose.yml -f docker-compose.postgres.yml up -d --force-recreate auth-service` buat reload env var-nya.

### Cutover `bikepacking.cyou` → redirect

Setelah semua di atas ke-test jalan (static page + `/auth/google/login` dari domain baru, bukan 404/cert error), server block `bikepacking.cyou` di-ganti total jadi redirect — gak serve apa-apa sendiri lagi:
```nginx
server {
    server_name bikepacking.cyou;

    return 301 https://taktikdansiasat.com$request_uri;

    listen [::]:443 ssl ipv6only=on;
    listen 443 ssl;
    ssl_certificate /etc/letsencrypt/live/bikepacking.cyou/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/bikepacking.cyou/privkey.pem;
    include /etc/letsencrypt/options-ssl-nginx.conf;
    ssl_dhparam /etc/letsencrypt/ssl-dhparams.pem;
}
```
Dilakuin paling akhir, bukan di awal — kalau domain baru belum bener-bener siap pas ini di-apply, user yang masih punya link lama gak bisa akses apa-apa sama sekali, gak ada fallback.

## Email transaksional (AWS SES)

`auth-service` kirim email "cara instal app" sekali doang, pas user baru pertama kali login lewat Google (bukan tiap login) — kode-nya di `src/email.rs`, dipanggil dari `routes/auth.rs::google_callback`. Pengirimnya `noreply@taktikdansiasat.com`, lewat SES, bukan Resend/provider lain — SES dipilih karena Resend sendiri jalan di atas SES, dan domain `taktikdansiasat.com` udah dipegang penuh jadi gak perlu beli hosting email buat verifikasi.

### Setup sekali doang di AWS Console
1. SES Console → Verified identities → tambah domain `taktikdansiasat.com`, verifikasi lewat DNS (tambahin CNAME DKIM yang dikasih SES ke Cloudflare DNS — bukan Hostinger, nameserver udah pindah ke Cloudflare).
2. Account dashboard → Dedicated IPs/pricing → pilih à la carte / pay-per-use (bukan plan "Essentials" yang ada minimum bulanan) — volume email di project ini kecil banget, pay-per-use ($0.10/1.000 email) jauh lebih murah.
3. Minta **production access** (keluar dari sandbox mode) — tanpa ini SES cuma bisa kirim ke alamat yang di-verifikasi manual satu-satu. Review AWS ~24 jam.
4. **IAM**: attach instance role ke EC2 instance (bukan access key statis di `.env`) dengan policy minimal:
   ```json
   {
     "Version": "2012-10-17",
     "Statement": [{
       "Effect": "Allow",
       "Action": ["ses:SendEmail", "ses:SendRawEmail"],
       "Resource": "*"
     }]
   }
   ```
   AWS SDK di `auth-service` (`aws-config`) otomatis resolve credentials dari instance role ini dan region dari IMDS — gak ada `AWS_*` env var yang perlu diisi di `.env` sama sekali.

### `APP_INSTALL_URL`

Env var ini nge-gate seluruh fitur: kalau unset, `google_callback` skip kirim email sama sekali (lihat `Config::app_install_url`). Sengaja dibikin begini karena kode SES ini ditulis duluan, sebelum project Expo-nya sendiri ada — jadi gak mungkin ada link asli buat dikirim. Begitu app Expo udah di-publish dan link `exp://...` atau `https://u.expo.dev/...`-nya ada, isi `APP_INSTALL_URL` di `.env` EC2 dan `docker compose ... up -d --force-recreate auth-service` — tanpa perlu ganti kode apa pun.

## Shared login dengan shop (repo `taktikdansiasat`, subdomain `shop.taktikdansiasat.com`)

Shop-nya repo terpisah (release cadence, deploy, dan database sendiri — liat README di repo itu buat alasannya), tapi user yang udah login di sini bisa langsung belanja tanpa login ulang. Nyambungnya cuma lewat dua hal, gak ada database atau kode yang di-share:

1. **`COOKIE_DOMAIN=taktikdansiasat.com`** di `.env` EC2 — bikin cookie sesi (`Config::cookie_domain`, diterapin di `routes/auth.rs::with_shared_domain`) ke-scope ke domain, bukan cuma host-nya doang, jadi otomatis kebawa browser ke `shop.taktikdansiasat.com` juga. **Wajib unset di local dev** — cookie yang di-scope ke domain asli gak pernah dikirim ke `localhost`.
2. **`JWT_SECRET` yang sama persis** di `.env` shop — shop cuma *verifikasi* token (baca cookie `session`, cek signature HS256 + `exp`, baca `role` dari claims), gak pernah nerbitin token sendiri. Login/logout/revoke session tetep cuma di `auth-service` ini.

Keterbatasan yang disadarin: shop gak ngecek status revoke session ke database (beda sama `AuthUser` extractor di sini yang round-trip ke tabel `sessions` — liat `auth/extractor.rs`). Jadi sesi yang di-revoke (misal abis "sign out everywhere") masih keanggep valid di shop sampe JWT-nya expired natural, walau di sini udah langsung ke-block. Trade-off yang diterima demi independensi — gak ada panggilan network/database silang servis buat tiap request shop.

## Migrasi data dari Supabase (sekali doang, pas cutover)

```bash
docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec -T postgres \
  pg_dump "<SUPABASE_SESSION_POOLER_URL>" -Fc -f /tmp/supabase_dump.custom

docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec -T postgres \
  pg_restore -d "postgres://bikepackid:<POSTGRES_PASSWORD>@localhost:5432/bikepackid" \
  --no-owner --no-privileges /tmp/supabase_dump.custom
```
**Restore harus ke database yang bener-bener kosong** (belum pernah di-migrate `sqlx` sendiri) — kalau auth-service udah sempat jalan duluan (auto-migrate bikin schema+constraint), `pg_restore` gagal karena urutan `COPY` data ketubruk foreign key yang udah aktif. Kalau kejadian: `DROP DATABASE bikepackid; CREATE DATABASE bikepackid OWNER bikepackid;` dulu, baru restore ulang. Error soal `supabase_vault` pas restore **aman diabaikan** (extension internal Supabase, gak dipakai aplikasi).

## Backup database

Postgres self-hosted gak punya managed backup bawaan (beda dari Supabase dulu) — ini tanggung jawab sendiri. `auth-service/scripts/backup-db.sh` + `restore-db.sh` udah disiapin, tinggal setup:

**1. Bucket R2 baru khusus backup** (Cloudflare dashboard) — **private**, beda dari `bikepackid-media` yang public-read buat foto. Generate scoped API token buat bucket ini doang (read+write).

**2. Install AWS CLI** (dipakai buat komunikasi ke R2, S3-compatible):
```bash
sudo apt install -y awscli
```

**3. Isi env var yang dibutuhin script** — taro di `~/.bashrc` atau file terpisah yang di-`source`, **bukan** di `.env` aplikasi (ini kredensial infra, beda dari kredensial aplikasi):
```bash
export R2_BACKUP_BUCKET=bikepackid-backups
export R2_BACKUP_ENDPOINT=https://<account_id>.r2.cloudflarestorage.com
export AWS_ACCESS_KEY_ID=<R2 token access key>
export AWS_SECRET_ACCESS_KEY=<R2 token secret key>
# Opsional — alert kalau backup gagal/gak jalan, daftar gratis di healthchecks.io
export HEALTHCHECK_PING_URL=https://hc-ping.com/<uuid-check-kamu>
```

**4. Jadwalin cron harian**:
```bash
crontab -e
# tambahin baris ini:
0 3 * * * . ~/.bashrc && /home/ubuntu/bikepackid/auth-service/scripts/backup-db.sh >> /var/log/bikepackid-backup.log 2>&1
```

**5. Test restore berkala** (bulanan, bukan cuma sekali pas setup) — backup yang gak pernah dites itu asumsi, bukan jaminan:
```bash
./scripts/restore-db.sh <nama-file-backup>.sql.gz
```
Restore ke database **terpisah** (`bikepackid_restore_test`, bukan `bikepackid` yang live), bandingin row count-nya sama database production, drop database test-nya setelah selesai cek.

## Perintah yang sering kepake

| Perintah | Fungsi |
|---|---|
| `docker compose -f docker-compose.yml -f docker-compose.postgres.yml ps` | status container |
| `... logs -f auth-service` | log realtime auth-service |
| `... logs -f postgres` | log realtime Postgres |
| `... restart auth-service` | restart tanpa pull ulang (gak baca ulang `.env`) |
| `... up -d` | recreate container kalau `.env` berubah |
| `... pull auth-service && ... up -d auth-service` | paksa deploy manual, gak nunggu Watchtower |
| `free -h` | cek RAM+swap |
| `sudo nginx -t` | cek syntax config sebelum reload |
| Console → EC2 → Connect → Session Manager | akses shell (bukan `ssh`) |

## Kalau perlu ganti instance type

`t4g.micro` (1GB) gak cukup buat auth-service+journey-service+Postgres bareng, itu sebabnya `t4g.small` (2GB) yang dipakai. Resize: **Stop instance** → **Actions → Instance settings → Change instance type** → **Start instance**. Elastic IP dan data EBS tetap sama, gak perlu setup ulang dari nol.
