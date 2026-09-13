# TODO sebelum production

Checklist buat sistem User (backend) yang sudah dibangun. Item lain (Journey/Checkpoint/marketplace) akan punya checklist sendiri begitu diimplementasikan — lihat `docs/SYSTEM_DESIGN.md`.

## Wajib sebelum live ke user beneran

- [ ] **Deploy di HTTPS** dan pastikan `COOKIE_SECURE` **tidak** di-set `false` (default-nya sudah `true`, cukup jangan di-override).
- [ ] **JWT_SECRET production** — generate baru yang panjang & random (`openssl rand -base64 48`), jangan pakai nilai dev. Simpan di secrets manager platform hosting (Railway/Render/Fly.io semua punya fitur env var terenkripsi), bukan file `.env` biasa di server.
- [ ] **Google OAuth consent screen** — submit ke Google buat verifikasi (mode "Testing" dibatasi ~100 user). Daftarkan juga redirect URI production di Google Cloud Console (client Web, dan Android/iOS kalau app native sudah jalan).
- [ ] **Rate limiting** di endpoint `/auth/google/login` dan `/auth/google/callback` — belum ada. Tanpa ini endpoint auth rawan disalahgunakan buat spam/DoS ringan (walau `pending_logins` sudah di-sweep otomatis, tetap perlu limit di level request).
- [ ] **Backup database** — belum ada strategi. Minimal: automated daily backup dari provider Postgres yang dipakai (Supabase/Neon/RDS biasanya punya built-in).
- [ ] **Sambungkan `OTEL_EXPORTER_OTLP_ENDPOINT` ke collector production** — kode-nya sudah siap (lihat bagian Observability di README), tinggal pilih & deploy tujuan (OpenTelemetry Collector, Kibana/Elastic APM, Datadog, Grafana Tempo, dst) dan set env var-nya. Tanpa ini trace tidak kemana-mana (cuma log stdout).
- [ ] **Alerting** — belum ada. `/health` (ping database) sudah ada buat dipakai orkestrator/load balancer, tapi belum ada yang mengirim alert kalau itu gagal. Setelah log/trace kekirim ke collector pilihan, set alert minimal buat: error rate naik, health check gagal, latency p99 endpoint auth melonjak.

## Penting, tapi bisa menyusul cepat setelah live

- [ ] **CI** — jalankan `cargo test`, `cargo audit`, dan `cargo clippy` otomatis tiap push/PR (belum ada workflow CI sama sekali).
- [ ] **Migrasi review** — pastikan proses deploy menjalankan `sqlx migrate run` terhadap DB production dengan aman (idealnya lewat CI/CD step terpisah, bukan otomatis saat app start di multi-instance, supaya tidak race kalau nanti scale ke >1 instance).
- [ ] **`pending_logins` di memory** — cuma aman selama backend jalan 1 instance. Begitu di-scale ke >1 instance (misal buat load balancing), pindahkan ke Redis atau state store bersama, karena in-memory map per-instance tidak akan konsisten antar instance.
- [ ] **`cargo audit` di CI** — jadwalkan reguler (bukan cuma sekali manual), karena RUSTSEC advisory baru terus muncul. Cek juga apakah pengecualian `RUSTSEC-2023-0071` di `.cargo/audit.toml` sudah ada fix upstream (lihat catatan di file itu).
- [ ] **Privasi data lokasi** — begitu fitur Journey/Checkpoint jalan (yang nyimpen lat/lng user), perlu kebijakan privasi jelas: siapa yang bisa lihat lokasi, retensi data, dan idealnya opsi "sembunyikan lokasi real-time" karena data lokasi itu sensitif.

## Nice to have (tidak blocking launch awal)

- [ ] Audit log viewer/admin UI buat baca tabel `audit_logs` (sekarang cuma bisa query manual).
- [ ] "Sign out everywhere" (revoke semua session milik satu user sekaligus) — tabel `sessions` sudah mendukung ini, tinggal tambah endpoint.
- [ ] Refresh token / access token jangka pendek + refresh jangka panjang, kalau nanti butuh model sesi yang lebih granular dari JWT 30 hari flat.
- [ ] Load testing endpoint auth sebelum ekspektasi traffic tinggi.

## Sudah beres

- [x] Login tanpa password (Google OAuth only, PKCE + CSRF state).
- [x] Session bisa di-revoke (tabel `sessions`, dicek tiap request).
- [x] Cookie session `httpOnly` + `Secure` (default) + `SameSite=Lax`.
- [x] Audit log login/logout.
- [x] `pending_logins` (in-memory, single-instance) auto-sweep percobaan login kedaluwarsa.
- [x] Dependency bebas kerentanan diketahui (`cargo audit` bersih, 1 pengecualian terdokumentasi).
- [x] Test coverage: unit test (JWT, role, config) + test terhadap DB asli (session, audit log) + test end-to-end lewat router (401/403/200 sesuai skenario).
- [x] Request ID per request (`x-request-id`, auto-generate, ikut di response header & semua log/span request itu).
- [x] Log terstruktur (JSON via `LOG_FORMAT=json`) + tiap request otomatis ke-log (method/path/status/latency).
- [x] Wiring OpenTelemetry/OTLP trace export (vendor-neutral) — tinggal arahkan `OTEL_EXPORTER_OTLP_ENDPOINT` ke collector pilihan saat production (lihat poin "Wajib" di atas).
- [x] `/health` — readiness check yang nge-ping database beneran, bukan 200 statis.
- [x] Graceful shutdown (SIGTERM/Ctrl+C) — request yang sedang jalan diselesaikan dulu, baru trace OTel di-flush, sebelum proses keluar.
- [x] HTTP client ke Google punya timeout (`connect_timeout` 5s, `timeout` 10s) — sebelumnya `reqwest::Client::new()` default tanpa timeout sama sekali, request bisa menggantung selamanya kalau Google lambat/hang.
- [x] Baris `sessions` yang sudah revoked/expired dibersihkan otomatis (background task tiap 6 jam) — sebelumnya tabel `sessions` tidak pernah dibersihkan sama sekali.
- [x] `AuthUser` extractor sekarang satu query yang sekaligus memverifikasi sesi itu benar-benar milik user di klaim JWT (bukan dua query terpisah yang implisit saling percaya).
- [x] User yang cancel di consent screen Google (`error=access_denied`) di-redirect halus ke frontend, bukan 400 mentah dengan pesan deserialisasi.
- [x] Error "percobaan login kedaluwarsa/tidak dikenal" sekarang 400 (kesalahan klien), terpisah dari error Google beneran down yang tetap 502 — sebelumnya keduanya dilaporkan sama.
- [x] `users.email` tidak lagi `UNIQUE` (migrasi 0004) — email yang didaur ulang antar akun Google berbeda tidak lagi bikin login gagal 500.
- [x] Masa berlaku cookie sesi diturunkan dari `expires_at` baris `sessions` (bukan konstanta terpisah yang bisa mencle dari nilai di database).
- [x] CORS mengizinkan header `Content-Type` — sebelumnya preflight buat request JSON (POST) akan gagal.
