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
   - **Image**: **Fedora** (pilih versi terbaru yang tersedia di daftar image Hetzner).
   - **Type**: CX22 (2 vCPU / 4GB RAM) — cukup buat Postgres + backend + Caddy jalan bareng. CX11 lebih murah tapi RAM-nya (2GB) agak mepet.
   - **SSH Key**: paste isi `~/.ssh/id_ed25519.pub` (bukan yang privat!).
3. Create & tunggu sampai server dapat IP publik.

## Fase 2 — Masuk & setup dasar server

```bash
ssh root@<IP_SERVER>
```
(Kalau `root@` ditolak, coba `ssh fedora@<IP_SERVER>` — sebagian image cloud Fedora pakai user `fedora` dengan `sudo`, bukan login root langsung. Kalau itu yang kejadian, tinggal tambahkan `sudo` di depan tiap perintah di bawah.)

Update sistem:
```bash
dnf upgrade --refresh -y
```

**Firewall** — Fedora pakai `firewalld` (beda dari `ufw` di Ubuntu/Debian yang saya sebut sebelumnya), biasanya sudah aktif secara default di image cloud-nya. Prinsipnya tetap sama: **tolak semua secara default, buka cuma yang benar-benar perlu**.

```bash
systemctl enable --now firewalld   # jaga-jaga kalau belum aktif
firewall-cmd --add-service=ssh --permanent   # biasanya sudah otomatis, aman diulang
firewall-cmd --add-port=8080/tcp --permanent  # backend, buat testing curl (Fase 1)
firewall-cmd --reload
firewall-cmd --list-all
```

**Install Docker:**
```bash
curl -fsSL https://get.docker.com | sh
systemctl enable --now docker
```
(Script resmi Docker ini mendeteksi Fedora otomatis dan install Docker Engine + Compose plugin. Wajar kalau kamu ingin baca isinya dulu sebelum `| sh`: `curl -fsSL https://get.docker.com` tanpa pipe, lihat isinya. Kalau script ini gagal/tidak support versi Fedora kamu, cek panduan resmi [Install Docker Engine on Fedora](https://docs.docker.com/engine/install/fedora/) sebagai alternatif — pakai `dnf` langsung dari repo Docker.)

## Fase 3 — Deploy aplikasinya

```bash
dnf install -y git
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
         - ./deploy/Caddyfile:/etc/caddy/Caddyfile:z
         - caddy_data:/data
   ```
   Tambahkan `caddy_data:` ke daftar `volumes:` di bawah.

   **Catatan khusus Fedora (SELinux):** perhatikan akhiran `:z` di baris volume `Caddyfile` — Fedora aktifkan SELinux *enforcing* secara default, dan tanpa `:z` itu, Docker biasanya kena `Permission denied` pas baca file yang di-*bind mount* dari host (beda dari volume `postgres_data`/`caddy_data` yang dikelola Docker sendiri, itu tidak kena masalah ini). `:z` bilang ke Docker "kasih label SELinux yang benar buat file ini boleh dibaca container". Kalau masih kena `Permission denied` juga, cek `sudo journalctl -u docker` atau `ausearch -m avc` buat lihat denial-nya persis apa.
3. **Hapus** `ports: ["8080:8080"]` dari service `backend` — biar backend cuma bisa diakses lewat Caddy, bukan langsung dari luar. Ini intinya reverse proxy: satu pintu masuk (Caddy, TLS di sini), bukan tiap service buka port sendiri-sendiri.
4. Edit `deploy/Caddyfile`, ganti `your-domain.com` dengan domain asli kamu. Caddy otomatis urus sertifikat TLS (Let's Encrypt) — tidak perlu setup manual.
5. Update `.env`: `FRONTEND_URL`, `GOOGLE_REDIRECT_URL` jadi `https://domain-kamu.com/...`, hapus baris `COOKIE_SECURE=false` (defaultnya `true`, pas buat HTTPS).
6. Daftarkan redirect URI baru (`https://domain-kamu.com/auth/google/callback`) di Google Cloud Console.
7. Update firewall: buka 80/443, tutup 8080 dari luar:
   ```bash
   firewall-cmd --add-service=http --permanent
   firewall-cmd --add-service=https --permanent
   firewall-cmd --remove-port=8080/tcp --permanent
   firewall-cmd --reload
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
| `firewall-cmd --list-all` | cek aturan firewall aktif |
| `sudo journalctl -u docker` | log Docker Engine (kalau `docker compose` aneh) |
