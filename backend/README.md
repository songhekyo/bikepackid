# bikepackid backend

Sistem User: login via Google OAuth, role-based access (web vs app).

## Jalankan lokal

1. Pastikan Postgres jalan, lalu buat database & user sesuai `DATABASE_URL` di `.env`.
2. Salin `.env.example` ke `.env` dan isi kredensial Google OAuth (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`) dari [Google Cloud Console](https://console.cloud.google.com/apis/credentials). Set `COOKIE_SECURE=false` hanya untuk dev lokal via HTTP — **wajib** `true` (atau dihapus, karena defaultnya `true`) di production.
3. `cargo run` — migrasi di `migrations/` jalan otomatis saat start.
4. `cargo test` — jalankan test (butuh koneksi ke database yang sama seperti `DATABASE_URL`).
5. `cargo audit` — scan kerentanan dependency (pengecualian yang didokumentasikan ada di `.cargo/audit.toml`).

## Struktur

- `src/config.rs` — baca konfigurasi dari env var.
- `src/models/user.rs` — struct `User` & enum `Role` (`viewer`/`creator`/`moderator`/`admin`/`superadmin`).
- `src/auth/google.rs` — client OAuth2 & fetch profil dari Google.
- `src/auth/jwt.rs` — issue/verify session token (JWT, disimpan di cookie httpOnly).
- `src/auth/session.rs` — session store di database (dipakai buat revoke token saat logout/ban, JWT sendiri tidak bisa dicabut).
- `src/auth/extractor.rs` — `AuthUser`, dipakai di handler untuk mewajibkan login; juga cek sesi belum di-revoke.
- `src/audit.rs` — catat event keamanan (login/logout, dst) ke tabel `audit_logs`.
- `src/routes/auth.rs` — `/auth/google/login`, `/auth/google/callback`, `/auth/logout`.
- `src/routes/me.rs` — `/me` (semua role login), `/app/status` (contoh route khusus `creator` ke atas).

## Keamanan

- Session token (JWT) disimpan di cookie `httpOnly`, `SameSite=Lax`, dan `Secure` (kecuali di-override lewat `COOKIE_SECURE=false` untuk dev lokal).
- Setiap token terikat ke baris `sessions` di database (`jti` claim) — logout/ban benar-benar mencabut akses, tidak cuma menghapus cookie di sisi client.
- Percobaan login yang tidak selesai (`pending_logins`, in-memory) otomatis dibersihkan setelah 10 menit supaya tidak numpuk di memori.
- Event login/logout tercatat di `audit_logs`.
- `cargo audit` bersih; satu pengecualian terdokumentasi di `.cargo/audit.toml` (RUSTSEC-2023-0071, `rsa` crate — terkunci di `Cargo.lock` sebagai kemungkinan dependency dari fitur `mysql` milik `sqlx-macros-core`, tapi tidak pernah benar-benar ter-compile karena kita cuma pakai fitur `postgres`; belum ada versi perbaikan dari upstream).

## Endpoint

| Method | Path | Auth | Keterangan |
|---|---|---|---|
| GET | `/auth/google/login` | - | redirect ke halaman login Google |
| GET | `/auth/google/callback` | - | tukar `code` dari Google, upsert user, set cookie sesi |
| POST | `/auth/logout` | - | hapus cookie sesi |
| GET | `/me` | wajib login | profil user yang sedang login |
| GET | `/app/status` | wajib login + role `creator`+ | contoh gate khusus app |
