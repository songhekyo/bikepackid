# Deploy ke Hetzner (VPS mentah)

Panduan ini sengaja manual — tujuannya bukan cuma "biar jalan", tapi biar kamu pegang langsung tiap lapisan: server Linux, firewall, Docker, reverse proxy, TLS. Jalankan tiap perintah di bawah satu-satu, jangan asal copy-paste semuanya sekaligus — biar kelihatan tiap langkah ngapain.

**Catatan jujur:** `Dockerfile`/`docker-compose.yml` di sini saya susun dengan pola standar (multi-stage build, cache layer dependency, dst) tapi **belum sempat saya `docker build` beneran** — environment saya kena blokir akses ke Docker Hub. Jadi ada kemungkinan kecil ada typo/error kecil pas kamu build pertama kali di server. Kalau ada error build, kirim log-nya ke saya, saya bantu benerin.

## Fase 0 — Siapkan SSH key (di laptop kamu, bukan di server)

```bash
ssh-keygen -t ed25519 -C "bikepackid-vps"
```

Ini bikin sepasang kunci: privat (`~/.ssh/id_ed25519`, jangan pernah dikirim ke mana pun) dan publik (`~/.ssh/id_ed25519.pub`, ini yang didaftarkan ke server). Nanti pas login SSH, server verifikasi kamu punya kunci privat yang cocok — lebih aman daripada password.

## Fase 1 — Provision server di Hetzner

1. Daftar di [console.hetzner.cloud](https://console.hetzner.cloud), buat project baru.
2. "Add Server":
   - **Location**: bebas (Falkenstein/Nuremberg Jerman, atau lokasi lain yang tersedia).
   - **Image**: Ubuntu 24.04.
   - **Type**: CX22 (2 vCPU / 4GB RAM) — cukup buat Postgres + backend + Caddy jalan bareng. CX11 lebih murah tapi RAM-nya (2GB) agak mepet.
   - **SSH Key**: paste isi `~/.ssh/id_ed25519.pub` (bukan yang privat!).
3. Create & tunggu sampai server dapat IP publik.

## Fase 2 — Masuk & setup dasar server

```bash
ssh root@<IP_SERVER>
```

Update sistem:
```bash
apt update && apt upgrade -y
```

**Firewall** — defaultnya server Hetzner tidak ada firewall aktif dari OS-nya (beda dari Security Group-nya cloud lain). Kita pasang `ufw` (Uncomplicated Firewall), dan prinsipnya: **tolak semua secara default, buka cuma yang benar-benar perlu**.

```bash
apt install -y ufw
ufw default deny incoming
ufw default allow outgoing
ufw allow 22/tcp        # SSH
ufw allow 8080/tcp      # backend, buat testing curl (Fase 1)
ufw enable
ufw status verbose
```

**Install Docker:**
```bash
curl -fsSL https://get.docker.com | sh
```
(Script resmi Docker — install Docker Engine + Compose plugin sekaligus. Wajar kalau kamu ingin baca isinya dulu sebelum `| sh`: `curl -fsSL https://get.docker.com` tanpa pipe, lihat isinya.)

## Fase 3 — Deploy aplikasinya

```bash
apt install -y git
git clone https://github.com/songhekyo/bikepackid.git
cd bikepackid/backend
```

Siapkan `.env` dari template:
```bash
cp .env.production.example .env
nano .env
```
Isi semua placeholder (`change-this-...`, `YOUR_SERVER_IP`, kredensial Google OAuth). Buat `JWT_SECRET` dan `POSTGRES_PASSWORD`, generate random:
```bash
openssl rand -base64 48
```

Build & jalankan:
```bash
docker compose up -d --build
```

Cek jalan atau tidak:
```bash
docker compose ps
docker compose logs -f backend
```
(`Ctrl+C` buat keluar dari log tanpa mematikan container-nya.)

Migrasi database jalan otomatis saat backend start (sama seperti lokal) — cek di log ada baris "bikepackid backend listening on port 8080".

## Fase 4 — Test curl

Dari server sendiri:
```bash
curl http://localhost:8080/health
```

Dari laptop kamu (ganti `<IP_SERVER>`):
```bash
curl http://<IP_SERVER>:8080/health
curl -i http://<IP_SERVER>:8080/auth/google/login
```

Kalau `/health` balas `ok` dan `/auth/google/login` balas redirect (302/303) ke `accounts.google.com` — deploy-nya berhasil.

**Login Google end-to-end belum bisa dites penuh di fase ini** — Google mewajibkan redirect URI yang didaftarkan cocok persis, dan untuk domain publik (bukan `localhost`) biasanya harus HTTPS. Itu alasan ada Fase 5 di bawah.

## Fase 5 (opsional, nanti) — Domain asli + HTTPS via Caddy

Baru kerjakan ini kalau sudah punya domain (atau subdomain) yang bisa diarahkan ke IP server.

1. Di DNS provider domain kamu, bikin **A record** menunjuk ke IP server Hetzner.
2. Tambahkan service `caddy` ke `docker-compose.yml`:
   ```yaml
     caddy:
       image: caddy:2
       restart: unless-stopped
       ports:
         - "80:80"
         - "443:443"
       volumes:
         - ./deploy/Caddyfile:/etc/caddy/Caddyfile
         - caddy_data:/data
   ```
   Tambahkan `caddy_data:` ke daftar `volumes:` di bawah.
3. **Hapus** `ports: ["8080:8080"]` dari service `backend` — biar backend cuma bisa diakses lewat Caddy, bukan langsung dari luar. Ini intinya reverse proxy: satu pintu masuk (Caddy, TLS di sini), bukan tiap service buka port sendiri-sendiri.
4. Edit `deploy/Caddyfile`, ganti `your-domain.com` dengan domain asli kamu. Caddy otomatis urus sertifikat TLS (Let's Encrypt) — tidak perlu setup manual.
5. Update `.env`: `FRONTEND_URL`, `GOOGLE_REDIRECT_URL` jadi `https://domain-kamu.com/...`, hapus baris `COOKIE_SECURE=false` (defaultnya `true`, pas buat HTTPS).
6. Daftarkan redirect URI baru (`https://domain-kamu.com/auth/google/callback`) di Google Cloud Console.
7. Update firewall: buka 80/443, tutup 8080 dari luar:
   ```bash
   ufw allow 80/tcp
   ufw allow 443/tcp
   ufw delete allow 8080/tcp
   ```
8. `docker compose up -d --build`

Setelah ini, alur login Google beneran bisa dites lengkap dari browser.

## Perintah yang sering kepake

| Perintah | Fungsi |
|---|---|
| `docker compose ps` | status container |
| `docker compose logs -f backend` | log realtime |
| `docker compose restart backend` | restart tanpa rebuild |
| `docker compose up -d --build` | rebuild & jalankan ulang (setelah `git pull` misalnya) |
| `docker compose down` | matikan semua (data Postgres tetap ada di volume) |
| `df -h` | cek sisa disk |
| `ufw status verbose` | cek aturan firewall aktif |
