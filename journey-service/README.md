# bikepackid journey-service

Modul Journey/Checkpoint/Post/Equipment/Sponsor — dipisah dari `auth-service` (dulu `backend`),
lihat [`docs/SYSTEM_DESIGN.md`](../docs/SYSTEM_DESIGN.md) buat desain lengkap. Per prinsip
"Monolith First" di dokumen itu: Journey, Checkpoint, Post, Equipment, dan Sponsor hidup sebagai
satu modul di crate ini (bukan dipecah lagi jadi service per-entity) — boundary-nya jelas lewat tabel sendiri
dan `user_can_edit_journey` sebagai satu-satunya tempat permission diputuskan, tapi belum ada
alasan konkret buat mecah lebih jauh dari "satu service terpisah dari auth-service."

## Jalankan lokal

**Migrasi database bukan tanggung jawab crate ini** — schema (`journeys`, `checkpoints`,
`posts`, `equipment_categories`, `journey_equipment`, `journey_sponsors`, dst) di-migrate oleh
`auth-service`
(`sqlx::migrate!` cuma dipanggil di sana). Jalanin `auth-service` sekali dulu terhadap
`DATABASE_URL` yang sama sebelum nyoba request apa pun ke `journey-service` — service ini gak
pernah migrate sendiri, cuma baca/tulis tabel yang udah dibikin `auth-service`.

1. Pastikan Postgres jalan dan migrasi `auth-service` udah diterapkan (lihat di atas).
2. Isi `.env`: `DATABASE_URL` (sama persis dengan punya `auth-service`), `JWT_SECRET` (**harus**
   sama persis — service ini verifikasi token yang `auth-service` keluarin, gak pernah
   keluarin token sendiri), `FRONTEND_URL`, `PORT` (default `8081`), `R2_ACCOUNT_ID`,
   `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET_NAME`, `R2_PUBLIC_URL_BASE`.
3. `cargo run`.
4. `cargo test` — butuh koneksi ke database yang sama seperti `DATABASE_URL`, termasuk migrasi
   `auth-service` udah jalan (beberapa test, misal yang ngecek kategori equipment ter-seed,
   gagal kalau migrasinya belum diterapkan). Dari workspace root, `cargo test --workspace` juga
   jalanin test `auth-service` dan `common`.

## Struktur

- `src/config.rs` — baca konfigurasi dari env var.
- `src/auth.rs` — `AuthUser`/`OptionalAuthUser` extractor, dengan salinan `authenticate`
  sendiri (bukan di-share dari `auth-service`) — lihat komentar di fungsi itu buat alasannya:
  ini batas service yang beneran, bukan duplikasi kebetulan.
- `src/journey/mod.rs` — struct/enum (`Journey`, `Checkpoint`, `Post`, `Equipment`,
  `EquipmentCategory`, `Sponsor`, dst) plus `JourneyListCache` (cache in-process buat
  `GET /journeys`).
- `src/journey/service.rs` — semua logic baca/tulis; ownership check diisolasi di satu fungsi
  (`user_can_edit_journey`), dipanggil dari tiap path tulis biar gak diulang-ulang.
- `src/journey/storage.rs` — client presigned-URL ke Cloudflare R2 (`rusty-s3`); service ini gak
  pernah nyentuh bytes file, cuma generate URL upload yang di-sign.
- `src/routes.rs` — semua HTTP handler + wiring `router()`.
- `src/state.rs` — `AppState`/`SharedState`.
- `src/test_support.rs` — helper test bersama (`insert_journey`, `insert_checkpoint`,
  `insert_equipment`, `insert_sponsor`, `cookie_for`, dst).

## Observability

Mekanisme sama persis dengan `auth-service` — lihat
[`auth-service/README.md`](../auth-service/README.md) bagian Observability buat detail lengkap
(request ID, log terstruktur, trace OpenTelemetry via Grafana Alloy). Satu beda: trace service
ini muncul di Grafana dengan `service.name = bikepackid_journey_service`, bukan
`bikepackid_auth_service`.

## Endpoint

| Method | Path | Auth | Keterangan |
|---|---|---|---|
| GET | `/health` | - | readiness check (ping database) |
| GET | `/version` | - | commit SHA baked in at build time |
| GET | `/journeys` | - (opsional) | feed publik, exclude `draft`, `?limit=&offset=`, di-cache in-process ~30 detik |
| GET | `/me/journeys` | wajib login | semua journey milik caller, termasuk `draft` |
| POST | `/journeys` | login + role `creator`+ | selalu mulai sebagai `draft` |
| GET | `/journeys/:id` | - (opsional) | 404 kalau `draft` dan viewer bukan owner/moderator+ |
| PATCH | `/journeys/:id` | login + role `creator`+, owner/moderator+ | field opsional (PATCH semantics) |
| GET | `/journeys/:id/checkpoints` | - (opsional) | aturan visibility sama kayak journey induknya |
| POST | `/journeys/:id/checkpoints` | login + role `creator`+, owner/moderator+ | `id` opsional dari client (idempotent retry) |
| GET | `/checkpoints/:id/posts` | - (opsional) | aturan visibility sama, diresolve lewat journey induknya |
| POST | `/checkpoints/:id/posts` | login + role `creator`+, owner/moderator+ | - |
| GET | `/journeys/:id/equipment` | - (opsional) | aturan visibility sama kayak checkpoint/post |
| POST | `/journeys/:id/equipment` | login + role `creator`+, owner/moderator+ | `category_id` wajib ada di `equipment_categories` |
| GET | `/equipment-categories` | - | daftar kategori gear, publik, gak di-scope ke journey |
| GET | `/journeys/:id/sponsors` | - (opsional) | aturan visibility sama kayak equipment |
| POST | `/journeys/:id/sponsors` | login + role `creator`+, owner/moderator+ | - |
| POST | `/uploads/presign-url` | login + role `creator`+ | presigned PUT URL R2 (15 menit), buat upload foto langsung dari client |
