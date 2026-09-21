# bikepackid backend

Sistem User: login via Google OAuth, role-based access (web vs app).

Konteks desain sistem secara keseluruhan (roadmap, entity yang belum diimplementasikan seperti Journey/Checkpoint/marketplace) ada di [`docs/SYSTEM_DESIGN.md`](../docs/SYSTEM_DESIGN.md). Checklist sebelum production ada di [`TODO_PRODUCTION.md`](./TODO_PRODUCTION.md). Panduan deploy ke VPS ada di [`DEPLOY.md`](./DEPLOY.md) — dokumentasi lengkap termasuk riwayat keputusan infra (provider, database, reverse proxy) di [`docs/INFRA_HISTORY.md`](../docs/INFRA_HISTORY.md).

## Jalankan lokal

1. Pastikan Postgres jalan, lalu buat database & user sesuai `DATABASE_URL` di `.env`.
2. Salin `.env.example` ke `.env` dan isi kredensial Google OAuth (`GOOGLE_CLIENT_ID`, `GOOGLE_CLIENT_SECRET`) dari [Google Cloud Console](https://console.cloud.google.com/apis/credentials). Set `COOKIE_SECURE=false` hanya untuk dev lokal via HTTP — **wajib** `true` (atau dihapus, karena defaultnya `true`) di production.
3. `cargo run` — migrasi di `migrations/` jalan otomatis saat start.
4. `cargo test` — jalankan test (butuh koneksi ke database yang sama seperti `DATABASE_URL`; test bikin & hapus baris sendiri, aman dijalankan berulang).
5. `cargo audit` — scan kerentanan dependency (pengecualian yang didokumentasikan ada di `.cargo/audit.toml`).

## Testing

Test tersebar di tiap modul (`#[cfg(test)] mod tests` di file yang sama, konvensi umum di Rust — bukan folder `tests/` terpisah karena crate ini binary, bukan library), plus helper bersama di `src/test_support.rs`.

- **Unit test murni** (tanpa DB): `auth/jwt.rs` (token valid/salah secret/expired), `models/user.rs` (`Role::can_use_app`), `config.rs` (parsing `COOKIE_SECURE`).
- **Test terhadap database asli**: `auth/session.rs` (create/revoke/authenticate, termasuk memastikan sesi tidak bisa dipakai buat autentikasi sebagai user lain), `audit.rs` (log tersimpan).
- **Test end-to-end lewat router** (`routes/mod.rs`, pakai `tower::ServiceExt::oneshot`, tanpa buka port beneran): `/me` tanpa cookie → 401, dengan cookie valid → 200, dengan session yang sudah di-revoke → 401 lagi; `/app/status` → 403 untuk `viewer`, 200 untuk `creator`; `/auth/google/login` → 5 request pertama dari 1 IP (disimulasikan lewat header `x-forwarded-for`) lolos, ke-6 kena 429, IP lain dapat kuota sendiri; `/auth/google/callback` → `error` dari Google di-relay mentah ke frontend untuk percobaan login yang valid, tapi 400 (bukan redirect) kalau `state`-nya tidak dikenal.

## Struktur

- `src/config.rs` — baca konfigurasi dari env var.
- `src/models/user.rs` — struct `User` & enum `Role` (`viewer`/`creator`/`moderator`/`admin`/`superadmin`).
- `src/auth/google.rs` — client OAuth2 & fetch profil dari Google.
- `src/auth/jwt.rs` — issue/verify session token (JWT, disimpan di cookie httpOnly).
- `src/auth/session.rs` — session store di database (revoke saat logout/ban, `authenticate` buat load user + validasi sesi dalam satu query, `purge_expired` buat dipanggil job pembersih).
- `src/auth/extractor.rs` — `AuthUser`, dipakai di handler untuk mewajibkan login; satu query yang sekaligus cek sesi belum di-revoke/expired dan memverifikasi sesi itu benar milik user di klaim JWT.
- `src/audit.rs` — catat event keamanan (login/logout, dst) ke tabel `audit_logs`.
- `src/routes/auth.rs` — `/auth/google/login`, `/auth/google/callback` (termasuk redirect halus kalau user cancel di consent screen Google), `/auth/logout`.
- `src/routes/me.rs` — `/me` (semua role login), `/app/status` (contoh route khusus `creator` ke atas).
- `src/routes/health.rs` — `/health`, readiness check yang benar-benar nge-ping database.
- `src/telemetry.rs` — setup logging + (opsional) export trace OpenTelemetry.
- `deploy/config.alloy` — config Grafana Alloy (collector OTLP lokal di production, lihat bagian Observability).

## Observability

- **Request ID**: tiap request dapat `x-request-id` (UUID, auto-generate kalau belum ada), dikembalikan di response header yang sama, dan tercatat di semua log/span request itu. Berguna buat lacak satu request lintas log.
- **Log terstruktur**: default human-readable buat dev lokal. Set `LOG_FORMAT=json` buat output JSON per baris (siap ditelan log shipper apa pun yang baca stdout — Filebeat/Vector buat ELK/Kibana, Datadog Agent, Fluent Bit, dst — tanpa perlu SDK vendor khusus buat logging).
- **Tiap request otomatis ke-log** (level INFO) lewat `TraceLayer`, isinya `method`, `path`, `request_id`, `status_code`, `latency_ms`.
- **Telemetry (trace) via OpenTelemetry/OTLP**: mati secara default (supaya dev lokal tidak butuh collector nyala). Set `OTEL_EXPORTER_OTLP_ENDPOINT` (misal `http://localhost:4318`) buat export trace lewat protokol OTLP — vendor-neutral, jalan ke OpenTelemetry Collector, Kibana/Elastic APM, Datadog, Grafana Tempo, Jaeger, Honeycomb, dll tanpa ganti kode aplikasi (tinggal ganti endpoint collector-nya). Backend selalu ngirim ke collector **lokal** (endpoint ini ditambah `/v1/traces` otomatis di kode, lihat `telemetry.rs`) — di production endpoint ini nunjuk ke **Grafana Alloy** (`deploy/config.alloy`, jalan sebagai service `alloy` di `docker-compose.yml`), yang baru forward ke Grafana Cloud pakai kredensial akun (`GRAFANA_CLOUD_*` di `.env`). Backend sendiri gak pernah pegang kredensial Grafana Cloud.
- **Batch span processor pakai runtime Tokio eksplisit** (`opentelemetry_sdk::trace::span_processor_with_async_runtime`, bukan `.with_batch_exporter()` default) — exporter HTTP-nya async (`reqwest-client`, bukan blocking), jadi loop export-nya harus jalan sebagai task Tokio beneran, bukan thread OS polos yang gak punya reactor.
- Atur verbosity log lewat `RUST_LOG` (standar `tracing`, misal `RUST_LOG=info,tower_http=debug`), default `info`.
- **Uptime monitoring**: UptimeRobot ping `/health` tiap 5 menit dari luar, kirim alert kalau gagal — di luar codebase ini (konfigurasi di dashboard UptimeRobot), dicatat di sini biar ke-track.

## Keamanan

- Session token (JWT) disimpan di cookie `httpOnly`, `SameSite=Lax`, dan `Secure` (kecuali di-override lewat `COOKIE_SECURE=false` untuk dev lokal). Masa berlaku cookie diturunkan langsung dari `expires_at` baris `sessions` (bukan konstanta terpisah yang bisa mencle dari yang di database).
- Setiap token terikat ke baris `sessions` di database (`jti` claim) — logout/ban benar-benar mencabut akses, tidak cuma menghapus cookie di sisi client. `AuthUser` extractor memverifikasi sesi itu milik user yang diklaim JWT (bukan cuma "sesi ini valid"), dalam satu query (`session::authenticate`).
- **`POST /auth/sign-out-everywhere`** — revoke semua sesi aktif milik user yang sedang login, bukan cuma sesi yang dipakai buat manggil endpoint ini. Berguna kalau user curiga ada sesi yang dicuri (device asing, malware, dll) dan mau cabut akses di mana pun tanpa harus tahu sesi mana yang harus dicabut satu-satu. Tercatat di `audit_logs` (`sign_out_everywhere`, termasuk jumlah sesi yang di-revoke).
- Percobaan login yang tidak selesai (`pending_logins`, in-memory) otomatis dibersihkan setelah 10 menit supaya tidak numpuk di memori.
- Kalau Google mengembalikan `error` (misal `access_denied` saat user cancel di consent screen) buat percobaan login yang **valid** (state dikenal & belum kedaluwarsa), backend relay nilainya apa adanya ke frontend (`{FRONTEND_URL}?error=<nilai>`) — backend tidak menafsirkan/melabeli sendiri (misal jadi `login=cancelled`); frontend yang menentukan artinya & UX-nya. Validasi `state` selalu terjadi **sebelum** cek `error`, supaya request dengan `state` sembarangan tidak bisa dapat nilai `error`-nya di-reflect balik (400 langsung).
- Event login/logout tercatat di `audit_logs`; kegagalan menulis log itu sendiri tidak silent — masuk `tracing::error!`.
- Baris `sessions` yang sudah revoked/expired lebih dari 7 hari dibersihkan otomatis oleh background task (`spawn_session_purge_task`, jalan tiap 6 jam) — tabel tidak tumbuh tanpa batas.
- HTTP client (ke Google) punya timeout eksplisit (`connect_timeout` 5s, `timeout` 10s) — Google lambat/hang tidak bisa menggantung request selamanya.
- Constraint `UNIQUE` di `users.email` sudah dilonggarkan jadi index biasa (migrasi 0004) — `google_id` (Google `sub`) yang jadi identitas asli; email yang didaur ulang antar akun Google berbeda tidak lagi bikin login gagal 500.
- **Rate limiting** di `/auth/google/login` dan `/auth/google/callback` (`tower_governor`): burst 5 request per IP, replenish 1 tiap 2 detik, lewat 429. Key IP diambil dari header `x-forwarded-for`/`x-real-ip`/`forwarded` (buat di belakang reverse proxy — Railway/Render/Fly semua set ini), fallback ke peer address asli kalau tidak ada proxy. **Penting**: header IP itu cuma boleh dipercaya kalau proxy di depannya yang men-set (dan men-strip nilai dari klien) — jangan expose server ini langsung ke internet tanpa proxy. In-memory per-instance, sama seperti `pending_logins` (lihat `TODO_PRODUCTION.md`).
- `cargo audit` bersih; satu pengecualian terdokumentasi di `.cargo/audit.toml` (RUSTSEC-2023-0071, `rsa` crate — terkunci di `Cargo.lock` sebagai kemungkinan dependency dari fitur `mysql` milik `sqlx-macros-core`, tapi tidak pernah benar-benar ter-compile karena kita cuma pakai fitur `postgres`; belum ada versi perbaikan dari upstream).

## Operasional

- **Graceful shutdown**: server menangkap Ctrl+C/SIGTERM, membiarkan request yang sedang jalan selesai sebelum proses keluar, baru setelah itu flush trace OTel yang masih ke-buffer.
- **`/health`** benar-benar nge-ping database (`SELECT 1`), bukan cuma return 200 statis — cocok buat readiness probe di Railway/Render/Fly/k8s.

## Endpoint

| Method | Path | Auth | Keterangan |
|---|---|---|---|
| GET | `/health` | - | readiness check (ping database) |
| GET | `/auth/google/login` | - | redirect ke halaman login Google |
| GET | `/auth/google/callback` | - | tukar `code` dari Google, upsert user, set cookie sesi |
| POST | `/auth/logout` | - | hapus cookie sesi |
| POST | `/auth/sign-out-everywhere` | wajib login | revoke semua sesi milik user, bukan cuma yang sedang dipakai |
| GET | `/me` | wajib login | profil user yang sedang login |
| GET | `/app/status` | wajib login + role `creator`+ | contoh gate khusus app |
