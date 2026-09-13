# bikepackid backend

Sistem User: login via Google OAuth, role-based access (web vs app).

## Jalankan lokal

1. Pastikan Postgres jalan, lalu buat database & user sesuai `DATABASE_URL` di `.env`.
2. Salin `.env.example` ke `.env` dan isi kredensial Google OAuth (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`) dari [Google Cloud Console](https://console.cloud.google.com/apis/credentials).
3. `cargo run` — migrasi di `migrations/` jalan otomatis saat start.

## Struktur

- `src/config.rs` — baca konfigurasi dari env var.
- `src/models/user.rs` — struct `User` & enum `Role` (`viewer`/`creator`/`moderator`/`admin`/`superadmin`).
- `src/auth/google.rs` — client OAuth2 & fetch profil dari Google.
- `src/auth/jwt.rs` — issue/verify session token (JWT, disimpan di cookie httpOnly).
- `src/auth/extractor.rs` — `AuthUser`, dipakai di handler untuk mewajibkan login.
- `src/routes/auth.rs` — `/auth/google/login`, `/auth/google/callback`, `/auth/logout`.
- `src/routes/me.rs` — `/me` (semua role login), `/app/status` (contoh route khusus `creator` ke atas).

## Endpoint

| Method | Path | Auth | Keterangan |
|---|---|---|---|
| GET | `/auth/google/login` | - | redirect ke halaman login Google |
| GET | `/auth/google/callback` | - | tukar `code` dari Google, upsert user, set cookie sesi |
| POST | `/auth/logout` | - | hapus cookie sesi |
| GET | `/me` | wajib login | profil user yang sedang login |
| GET | `/app/status` | wajib login + role `creator`+ | contoh gate khusus app |
