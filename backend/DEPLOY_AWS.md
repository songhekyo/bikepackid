# Deploy ke AWS EC2 — panduan yang live sekarang

Menggantikan [`DEPLOY.md`](./DEPLOY.md) (VPS Nusa/Fedora) sebagai panduan operasional aktif, sejak cutover 28 September 2026. `DEPLOY.md` dibiarin ada sebagai referensi historis — banyak prinsipnya (Watchtower, Grafana Alloy, nginx reverse-proxy) tetep sama persis, cuma OS-level command yang beda. Rasional lengkap kenapa pindah ke AWS, kenapa pilih Graviton/`arm64`, dan kenapa Postgres balik self-hosted (bukan Supabase) ada di [`docs/AWS_MIGRATION.md`](../docs/AWS_MIGRATION.md); bug-bug yang ketemu pas eksekusi ada di [`docs/INFRA_HISTORY.md`](../docs/INFRA_HISTORY.md) bagian 12.

Live di `https://bikepacking.cyou`, EC2 `t4g.small` (2 vCPU/2GB RAM, ARM Graviton, Ubuntu 26.04), region `ap-southeast-2` Sydney.

## Akses instance: SSM Session Manager, bukan SSH biasa

IP publik laptop kamu kemungkinan berubah-ubah (ISP dynamic, ganti jaringan) — daripada terus update rule "My IP" di security group, akses instance lewat **AWS Console → EC2 → Instances → pilih instance → Connect → tab Session Manager**. Ini gak butuh key/IP whitelist sama sekali (otentikasi IAM). Port 22 di security group udah dicabut.

Begitu masuk, switch ke user `ubuntu` (session default-nya `ssm-user`):
```bash
sudo su - ubuntu
cd ~/bikepackid/backend
```

## Setup awal (sekali doang, pas provisioning instance baru)

Provisioning EC2 (VPC, Security Group, instance type, Elastic IP) — lihat `docs/AWS_MIGRATION.md` Fase 1. Setelah instance `Running`:

```bash
sudo apt update && sudo apt full-upgrade -y

# Swap wajib — instance kecil (t4g.small = 2GB) jalanin backend + Postgres
# bareng, RAM ketat sama kayak alasan swap di VPS lama.
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
cd bikepackid/backend
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
docker compose -f docker-compose.yml -f docker-compose.postgres.yml logs -f backend
```
Cari `bikepackid backend listening on port 8080` tanpa `panicked at ...`. Setelah ini, **Watchtower yang jalanin update-nya otomatis** — sama persis kayak VPS lama, gak ada bedanya (Watchtower gak peduli host-nya di mana).

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

## Migrasi data dari Supabase (sekali doang, pas cutover)

```bash
docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec -T postgres \
  pg_dump "<SUPABASE_SESSION_POOLER_URL>" -Fc -f /tmp/supabase_dump.custom

docker compose -f docker-compose.yml -f docker-compose.postgres.yml exec -T postgres \
  pg_restore -d "postgres://bikepackid:<POSTGRES_PASSWORD>@localhost:5432/bikepackid" \
  --no-owner --no-privileges /tmp/supabase_dump.custom
```
**Restore harus ke database yang bener-bener kosong** (belum pernah di-migrate `sqlx` sendiri) — kalau backend udah sempat jalan duluan (auto-migrate bikin schema+constraint), `pg_restore` gagal karena urutan `COPY` data ketubruk foreign key yang udah aktif. Kalau kejadian: `DROP DATABASE bikepackid; CREATE DATABASE bikepackid OWNER bikepackid;` dulu, baru restore ulang. Error soal `supabase_vault` pas restore **aman diabaikan** (extension internal Supabase, gak dipakai aplikasi).

## Perintah yang sering kepake

| Perintah | Fungsi |
|---|---|
| `docker compose -f docker-compose.yml -f docker-compose.postgres.yml ps` | status container |
| `... logs -f backend` | log realtime backend |
| `... logs -f postgres` | log realtime Postgres |
| `... restart backend` | restart tanpa pull ulang (gak baca ulang `.env`) |
| `... up -d` | recreate container kalau `.env` berubah |
| `... pull backend && ... up -d backend` | paksa deploy manual, gak nunggu Watchtower |
| `free -h` | cek RAM+swap |
| `sudo nginx -t` | cek syntax config sebelum reload |
| Console → EC2 → Connect → Session Manager | akses shell (bukan `ssh`) |

## Kalau perlu ganti instance type

`t4g.micro` (1GB) gak cukup buat backend+Postgres bareng, itu sebabnya `t4g.small` (2GB) yang dipakai. Resize: **Stop instance** → **Actions → Instance settings → Change instance type** → **Start instance**. Elastic IP dan data EBS tetap sama, gak perlu setup ulang dari nol.
