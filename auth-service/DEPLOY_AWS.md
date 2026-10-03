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

## Landing page statis (`web/`)

Halaman "coming soon" (`web/index.html` di root repo ini) di-serve **langsung oleh nginx dari hasil `git pull`** — bukan lewat Docker, bukan bagian dari `auth-service`/`journey-service`. Alasannya: server udah ngejalanin `git pull` di `~/bikepackid` tiap deploy (lihat bagian rename di atas), jadi file statis yang numpuk di repo otomatis ter-update di server tanpa langkah ekstra — gak perlu rebuild image atau sync manual.

Favicon-nya inline sebagai `data:image/svg+xml` di `<head>` halaman itu sendiri — gak ada file terpisah (`frog.svg` yang tadinya ada udah dihapus), jadi cuma butuh **satu** `location` exact-match di `/etc/nginx/sites-available/bikepackid.conf` (urutan gak masalah relatif ke `location /`/regex lain — exact match `=` selalu menang duluan di nginx):
```nginx
    location = / {
        root /home/ubuntu/bikepackid/web;
        try_files /index.html =404;
    }
```
lalu `sudo nginx -t && sudo systemctl reload nginx`. Setelahnya `/` nyajiin halaman statis ini, sisanya (`/auth/google/login`, `/me`, `/journeys`, dst) tetep ke-proxy seperti biasa — gak ada yang berubah dari rule proxy yang udah ada.

**Tombol "Masuk dengan Google"** di halaman ini ngarah ke `/auth/google/login` (rute asli di `auth-service`, bukan `/auth/google`) — udah dicek cocok.

**Rebrand ke "Taktik dan Siasat"**: halaman ini sempat ganti isi dua kali (brand "bikepacking.id" → "Taktik dan Siasat", mascot frog → heart) setelah PR awalnya dibuat — konten final ada di `web/index.html`, gak ada perubahan ke cara serve-nya. Domain final rencananya `taktikdansiasat.com` (udah dibeli), tapi migrasi DNS/Cloudflare Pages/OAuth redirect URI belum dieksekusi — `bikepacking.cyou` yang sekarang masih yang live sampai itu beres (lihat diskusi di sesi, belum didokumentasiin di sini).

**Belum ada**: endpoint `/api/seats` dan `/api/waitlist` yang dipanggil script di halaman ini — `/api/seats` gagal dengan aman (fallback ke "20 kursi tersisa" statis), tapi form waitlist bakal 404 beneran kalau disubmit. Belum dibangun, di luar scope nambahin halaman statis ini.

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
