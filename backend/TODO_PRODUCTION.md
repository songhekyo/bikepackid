# TODO sebelum production

Checklist buat sistem User (backend) yang sudah dibangun. Item lain (Journey/Checkpoint/marketplace) akan punya checklist sendiri begitu diimplementasikan — lihat `docs/SYSTEM_DESIGN.md`.

## Wajib sebelum live ke user beneran

- [ ] **Google OAuth consent screen** — masih mode "Testing" (dibatasi ~100 user, didaftarin manual). Redirect URI production sudah benar terdaftar (`https://bikepacking.cyou/auth/google/callback`); yang belum: submit consent screen ke Google buat verifikasi kalau mau user di luar daftar testing bisa login.
- [ ] **Backup database** — belum ada strategi, dan sekarang **lebih mendesak** dari sebelumnya: sejak cutover ke AWS (`docs/INFRA_HISTORY.md` bagian 12), Postgres jalan **self-hosted** di container EC2, bukan Supabase lagi — gak ada lagi automated backup bawaan provider. Minimal: cron job `pg_dump` terjadwal ke luar instance (misal ke Cloudflare R2 yang udah dipakai buat media), bukan cuma andelin EBS snapshot manual.
- [ ] **Alerting berbasis metric/trace** — uptime check sudah ada (lihat "Sudah beres"), tapi belum ada alert buat error rate naik atau latency p99 endpoint auth melonjak. Datanya sudah masuk Grafana Cloud (trace), tinggal bikin alert rule di Grafana buat kondisi-kondisi itu.

## Penting, tapi bisa menyusul cepat setelah live

- [ ] **Migrasi review** — pastikan proses deploy menjalankan `sqlx migrate run` terhadap DB production dengan aman (idealnya lewat CI/CD step terpisah, bukan otomatis saat app start di multi-instance, supaya tidak race kalau nanti scale ke >1 instance).
- [ ] **`pending_logins` dan rate limiter di memory** — keduanya cuma aman selama backend jalan 1 instance. Begitu di-scale ke >1 instance (misal buat load balancing), tiap instance punya kuota rate-limit sendiri-sendiri (efektifnya limit riil jadi N× lebih longgar) dan `pending_logins` tidak konsisten antar instance. Pindahkan ke Redis atau state store bersama kalau sudah butuh multi-instance.
- [ ] **`cargo audit` terjadwal** — `.github/workflows/ci.yml` udah jalanin `cargo audit` tiap push/PR, tapi itu cuma ke-trigger kalau ada kode yang berubah; RUSTSEC advisory baru bisa muncul kapan aja buat `Cargo.lock` yang udah lama gak disentuh. Tambahin trigger `schedule` (cron mingguan) di workflow yang sama biar tetep ke-cek walau gak ada push. Cek juga apakah pengecualian `RUSTSEC-2023-0071` di `.cargo/audit.toml` sudah ada fix upstream (lihat catatan di file itu).
- [ ] **Privasi data lokasi** — begitu fitur Journey/Checkpoint jalan (yang nyimpen lat/lng user), perlu kebijakan privasi jelas: siapa yang bisa lihat lokasi, retensi data, dan idealnya opsi "sembunyikan lokasi real-time" karena data lokasi itu sensitif.

## Nice to have (tidak blocking launch awal)

- [ ] Audit log viewer/admin UI buat baca tabel `audit_logs` (sekarang cuma bisa query manual).
- [ ] Refresh token / access token jangka pendek + refresh jangka panjang, kalau nanti butuh model sesi yang lebih granular dari JWT 30 hari flat.
- [ ] Load testing endpoint auth sebelum ekspektasi traffic tinggi.

## Sudah beres

- [x] **CI** — `.github/workflows/ci.yml` jalanin `cargo test`, `cargo clippy`, `cargo audit` tiap push/PR, plus build (native `arm64`) & push image Docker ke GHCR tiap push ke `main` — server tinggal `docker compose pull`, gak pernah compile Rust sendiri lagi (lihat `DEPLOY_AWS.md`).
- [x] Login tanpa password (Google OAuth only, PKCE + CSRF state).
- [x] Session bisa di-revoke (tabel `sessions`, dicek tiap request).
- [x] Cookie session `httpOnly` + `Secure` (default) + `SameSite=Lax`.
- [x] Audit log login/logout.
- [x] `pending_logins` (in-memory, single-instance) auto-sweep percobaan login kedaluwarsa.
- [x] Dependency bebas kerentanan diketahui (`cargo audit` bersih, 1 pengecualian terdokumentasi).
- [x] Test coverage: unit test (JWT, role, config) + test terhadap DB asli (session, audit log) + test end-to-end lewat router (401/403/200 sesuai skenario).
- [x] Request ID per request (`x-request-id`, auto-generate, ikut di response header & semua log/span request itu).
- [x] Log terstruktur (JSON via `LOG_FORMAT=json`) + tiap request otomatis ke-log (method/path/status/latency).
- [x] Trace export OpenTelemetry/OTLP **live** ke Grafana Cloud lewat Grafana Alloy (`deploy/config.alloy`) — bukan cuma wiring, sudah dikonfirmasi trace beneran masuk. Lihat `docs/INFRA_HISTORY.md` bagian Observability buat riwayat 3 bug yang sempat nutupin ini.
- [x] Uptime monitoring — UptimeRobot ping `/health` tiap 5 menit dari luar, alert email kalau gagal.
- [x] Deploy di HTTPS (`https://bikepacking.cyou`, nginx + certbot) — `COOKIE_SECURE` pakai default `true`, tidak di-override.
- [x] `JWT_SECRET` production — di-generate random (`openssl rand -base64 48`) saat setup `.env` di server, bukan nilai dev.
- [x] `/health` — readiness check yang nge-ping database beneran, bukan 200 statis.
- [x] Graceful shutdown (SIGTERM/Ctrl+C) — request yang sedang jalan diselesaikan dulu, baru trace OTel di-flush, sebelum proses keluar.
- [x] HTTP client ke Google punya timeout (`connect_timeout` 5s, `timeout` 10s) — sebelumnya `reqwest::Client::new()` default tanpa timeout sama sekali, request bisa menggantung selamanya kalau Google lambat/hang.
- [x] Baris `sessions` yang sudah revoked/expired dibersihkan otomatis (background task tiap 6 jam) — sebelumnya tabel `sessions` tidak pernah dibersihkan sama sekali.
- [x] `AuthUser` extractor sekarang satu query yang sekaligus memverifikasi sesi itu benar-benar milik user di klaim JWT (bukan dua query terpisah yang implisit saling percaya).
- [x] User yang cancel di consent screen Google (`error=access_denied`) di-redirect halus ke frontend, bukan 400 mentah dengan pesan deserialisasi. Backend cuma relay nilai `error` apa adanya (tidak melabeli sendiri jadi "cancelled") — frontend yang menentukan arti & UX-nya. Validasi `state` (harus percobaan login yang dikenal & belum kedaluwarsa) selalu jalan duluan sebelum `error` dibaca, supaya `error` tidak bisa di-reflect balik lewat `state` sembarangan.
- [x] Error "percobaan login kedaluwarsa/tidak dikenal" sekarang 400 (kesalahan klien), terpisah dari error Google beneran down yang tetap 502 — sebelumnya keduanya dilaporkan sama.
- [x] `users.email` tidak lagi `UNIQUE` (migrasi 0004) — email yang didaur ulang antar akun Google berbeda tidak lagi bikin login gagal 500.
- [x] Masa berlaku cookie sesi diturunkan dari `expires_at` baris `sessions` (bukan konstanta terpisah yang bisa mencle dari nilai di database).
- [x] CORS mengizinkan header `Content-Type` — sebelumnya preflight buat request JSON (POST) akan gagal.
- [x] Rate limiting per-IP di `/auth/google/login` dan `/auth/google/callback` (`tower_governor`, burst 5 / replenish 1 tiap 2 detik, key dari `x-forwarded-for`/`x-real-ip`/`forwarded` dengan fallback ke peer IP). In-memory per-instance — lihat catatan multi-instance di atas.
- [x] "Sign out everywhere" (`POST /auth/sign-out-everywhere`) — revoke semua sesi milik user yang sedang login sekaligus, bukan cuma sesi yang dipakai manggil endpoint-nya. Tercatat di `audit_logs`.
