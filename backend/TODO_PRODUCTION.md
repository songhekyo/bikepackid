# TODO sebelum production

Checklist buat sistem User (backend) yang sudah dibangun. Item lain (Journey/Checkpoint/marketplace) akan punya checklist sendiri begitu diimplementasikan — lihat `docs/SYSTEM_DESIGN.md`.

## Wajib sebelum live ke user beneran

- [ ] **Deploy di HTTPS** dan pastikan `COOKIE_SECURE` **tidak** di-set `false` (default-nya sudah `true`, cukup jangan di-override).
- [ ] **JWT_SECRET production** — generate baru yang panjang & random (`openssl rand -base64 48`), jangan pakai nilai dev. Simpan di secrets manager platform hosting (Railway/Render/Fly.io semua punya fitur env var terenkripsi), bukan file `.env` biasa di server.
- [ ] **Google OAuth consent screen** — submit ke Google buat verifikasi (mode "Testing" dibatasi ~100 user). Daftarkan juga redirect URI production di Google Cloud Console (client Web, dan Android/iOS kalau app native sudah jalan).
- [ ] **Rate limiting** di endpoint `/auth/google/login` dan `/auth/google/callback` — belum ada. Tanpa ini endpoint auth rawan disalahgunakan buat spam/DoS ringan (walau `pending_logins` sudah di-sweep otomatis, tetap perlu limit di level request).
- [ ] **Backup database** — belum ada strategi. Minimal: automated daily backup dari provider Postgres yang dipakai (Supabase/Neon/RDS biasanya punya built-in).
- [ ] **Monitoring & alerting** — saat ini cuma `tracing`/log ke stdout. Perlu structured logging + tempat nampung log (misal ke provider hosting atau Axiom/Grafana Loki), dan alert kalau error rate naik atau server down.

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
