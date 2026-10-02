# Journey — implementation TODO

Breakdown kerja buat 4 task yang udah di-track (lihat sesi kerja, task #0-#3), urut sesuai dependency: task 0 (infra) harus kelar duluan sebelum task 1 mulai ngoding (butuh credential R2 buat upload foto). Task 2 dan 3 sama-sama nunggu task 1 kelar (butuh tabel `journeys` ada duluan), tapi 2 dan 3 sendiri gak saling gantung. Rujukan desain lengkap ada di [`SYSTEM_DESIGN.md`](./SYSTEM_DESIGN.md); rujukan implementasi detail (file per file) ada di plan file sesi ini.

## Task 0 — Setup infra Cloudflare R2 (prasyarat, sebelum ngoding)

Kebanyakan langkah ini dilakuin **manual di dashboard Cloudflare** — gak bisa diotomatisin dari sini, sama kayak setup VPS/Supabase/GHCR sebelumnya. Saya kasih instruksi persis pas kita mulai kerjain task ini.

- [ ] Bikin akun Cloudflare (gratis) kalau belum ada
- [ ] Bikin R2 bucket baru (misal `bikepackid-media`)
- [ ] Generate R2 API token (Access Key ID + Secret Access Key) — scoped **cuma** ke bucket ini (read+write), bukan full account access
- [ ] Catat R2 endpoint (`https://<account_id>.r2.cloudflarestorage.com`)
- [ ] Aktifkan public access ke bucket buat baca (r2.dev public URL dulu — gratis, cukup buat mulai; custom domain kayak `media.bikepacking.cyou` bisa nyusul, tinggal ganti prefix URL doang, gak breaking)
- [ ] Tambah ke `.env`/`.env.production.example`: `R2_ACCOUNT_ID`, `R2_ACCESS_KEY_ID`, `R2_SECRET_ACCESS_KEY`, `R2_BUCKET_NAME`, `R2_PUBLIC_URL_BASE`
- [ ] **Perlakuan credential**: sama kayak DB password/API key lain di project ini — jangan paste mentah ke chat, rotate kalau kepaste gak sengaja

**Catatan batasan yang diterima (bukan bug)**: bucket public buat baca berarti siapa aja yang tau/nebak URL objek bisa akses foto, termasuk punya journey yang masih `draft` — tapi karena nama file objeknya random (UUID), ini "security by obscurity" yang wajar dipakai banyak app buat kasus kayak gini (bedain dari soal *visibility* journey di database yang tetep ketat lewat `visible_checkpoints`/`visible_posts` VIEW). Solusi yang lebih ketat (signed read URL) butuh proxy baca lewat backend — nge-reintroduce beban VPS yang justru mau dihindarin, jadi sengaja gak dilakuin.

**Belajar SRE (nyusul, gak blocking)**: setup pertama tetep manual di dashboard (paling cepat buat dapetin credential-nya). Begitu udah jalan, worth di-Terraform-in sebagai latihan — `cloudflare_r2_bucket` resource, scope cuma buat R2 ini (jangan retroactively nulis ulang VPS/Supabase/GHCR yang udah stabil). Local state dulu, remote state jadi latihan lanjutan kalau mau.

## Task 1 — Journey/Checkpoint/Post core module

Loop inti: create journey → checkpoint → post. Semua yang lain nunggu ini kelar duluan.

### Migrasi
- [x] `0005_create_journeys.sql` — tabel `journeys` (status enum 4 nilai: `draft`/`planning`/`published`/`archived`; `start_lat`/`start_lng`/`end_lat`/`end_lng` nullable; `seeking_sponsor` boolean default `false`; `donation_url` nullable; `CHECK` constraint: wajib ada start/end lat-lng begitu keluar dari `draft`)
- [x] `0006_create_checkpoints.sql` — tabel `checkpoints` (status enum `published`/`flagged`/`removed`; `trigger_type` `manual`/`retroactive`)
- [x] `0007_create_posts.sql` — tabel `posts` (`checkpoint_id` NOT NULL — keputusan final, gak ada post level-journey; status enum sama kayak checkpoint)
- [x] `0008_create_visibility_views.sql` — `visible_checkpoints` dan `visible_posts` (VIEW, bukan materialized) — harus setelah 3 tabel di atas karena join ke semuanya

### Kode Rust
- [x] `src/error.rs` — tambah varian `AppError::NotFound` → `StatusCode::NOT_FOUND`
- [x] `src/journey/mod.rs` — struct `Journey`/`Checkpoint`/`Post` + enum status (`sqlx::FromRow`, `Serialize`, pola sama kayak `models/user.rs`)
- [x] `src/journey/service.rs` — `create_journey`, `get_journey`, `list_public_journeys` (pagination `limit`/`offset`, clamp maks 50), `update_journey`, `create_checkpoint`, `list_checkpoints`, `create_post`, `list_posts`; ownership check (`user_id` cocok atau role `moderator`+) diisolasi di satu fungsi biar gampang di-extend pas ada collaborator nanti
- [x] `src/routes/journey.rs` — handler tipis, pola sama kayak `routes/me.rs`; reads public (query ke `visible_checkpoints`/`visible_posts`, bukan tabel mentah), writes butuh `AuthUser` + `role.can_use_app()`
- [x] `src/routes/mod.rs` — `pub mod journey;` + wire route (`/journeys`, `/journeys/:id`, `/journeys/:id/checkpoints`, `/checkpoints/:id/posts`, `/uploads/presign-url`)
- [x] `src/main.rs` — `mod journey;`

### Upload foto (perlu Task 0 kelar duluan)
- [x] `Cargo.toml` — dependency S3-compatible client (`rusty-s3`, presigned-URL-only — lebih ringan dari `aws-sdk-s3` buat build time)
- [x] `AppState` — tambah field client/config R2
- [x] Endpoint `POST /uploads/presign-url` — return presigned PUT URL, App upload foto langsung ke R2, backend cuma nyimpen URL publik hasilnya ke `posts.media_url`/`journeys.cover_image` setelah upload sukses
- [x] `post.type` tambah `'photo'` di enum

### Checkpoint ID (buat offline-sync nanti)
- [x] `create_checkpoint` nerima `id` opsional dari client (bukan asumsi server yang selalu generate) — App (creator) bakal butuh ini pas offline-sync dibangun, jangan sampai jadi breaking change belakangan

### Test
- [x] `test_support.rs` — `insert_journey(pool, user_id, status)`, `insert_checkpoint(pool, journey_id)`
- [x] Unit test `journey/service.rs`: create+get roundtrip; `list_public_journeys` exclude draft, include planning/published/archived; checkpoint di journey draft gak nongol di list publik walau checkpoint-nya sendiri `published`; ownership check nolak non-owner non-moderator
- [x] Router test `routes/mod.rs`: `POST /journeys` viewer→403, creator→201; `GET /journeys/:id` draft ke non-owner→404; `PATCH /journeys/:id` beda creator→403, moderator→200

### Docs & verifikasi
- [x] `backend/README.md` — endpoint table + Struktur list (`src/journey/`)
- [x] `cargo build` / `cargo test` / `cargo clippy --all-targets -- -D warnings` bersih
- [x] Smoke test: CHECK constraint diverifikasi manual di Postgres (draft insert tanpa lat/lng lolos, planning insert tanpa lat/lng ditolak DB)
- [x] Commit, push, PR (nyambung ke PR #17 yang masih kebuka)

## Task 2 — Journey Equipment (nunggu Task 1)

Daftar gear (sepeda, kamera, helm, tenda, dll), informational, seam murah ke commerce.

### Migrasi
- [x] `0009_create_equipment_categories.sql` — lookup table + seed data awal (Sepeda, Ban, Groupset, Tas, Kamera, Helm, Tenda, Kompor)
- [x] `0010_create_journey_equipment.sql` — `category_id` FK, `name`, `brand` nullable, `product_url` nullable, `notes` nullable, status enum (`published`/`flagged`/`removed`), plus `visible_equipment` view (sama pola kayak `visible_checkpoints`/`visible_posts`)

### Kode Rust
- [x] Struct `Equipment`/`EquipmentCategory` (di `journey-service/src/journey/mod.rs`, bareng Checkpoint/Post — gak dipecah ke file sendiri)
- [x] Service function: `create_equipment`, `list_equipment`, `list_equipment_categories`
- [x] Route: `GET/POST /journeys/:id/equipment` — permission sama kayak checkpoint/post (owner journey atau moderator+); plus `GET /equipment-categories` (publik, gak di-scope ke journey)

### Test & docs
- [x] Unit + router test (pola sama kayak Task 1) — 10 test baru, semua lolos terhadap Postgres asli
- [x] `journey-service` docs update — `journey-service/README.md` dibikin dari nol (belum pernah ada sebelumnya), isinya semua endpoint yang ada sekarang, bukan cuma equipment
- [x] Commit, push, PR

## Task 3 — Journey Sponsors (nunggu Task 1, paralel sama Task 2)

Daftar sponsor yang udah deal (di luar platform), ditampilin di halaman journey.

### Migrasi
- [x] `0011_create_journey_sponsors.sql` — `name`, `logo_url` nullable, `website_url` nullable, `notes` nullable, status enum, plus `visible_sponsors` view (sama pola kayak `visible_checkpoints`/`visible_posts`/`visible_equipment`)

### Kode Rust
- [x] Struct `Sponsor` (di `journey-service/src/journey/mod.rs`, bareng Checkpoint/Post/Equipment — gak dipecah ke file sendiri)
- [x] Service function: `create_sponsor`, `list_sponsors`
- [x] Route: `GET/POST /journeys/:id/sponsors` — permission sama kayak equipment (owner journey atau moderator+)

### Test & docs
- [x] Unit + router test (pola sama kayak Task 2) — 7 test baru, semua lolos terhadap Postgres asli
- [x] `journey-service` docs update — `journey-service/README.md` (endpoint table + Struktur list)
- [ ] Commit, push, PR

## Task 4 — Fitur konektivitas ala-telco (nunggu App mobile ada, belum di-desain detail)

Empat fitur ini dipilih spesifik karena nyambung ke masalah nyata bikepacker (sering kehilangan sinyal data di rute terpencil) **dan** sekalian jadi vehicle belajar skill yang mirip domain telco (SMS gateway, sync atas koneksi gak stabil, push delivery, geospasial). Belum ada desain schema/API detail — ini baru daftar scope, bukan checklist implementasi kayak Task 1-3.

- [ ] **SMS fallback / emergency check-in** — checkpoint via SMS pas gak ada sinyal data (cuma sinyal 2G/suara). Butuh integrasi SMS gateway (Twilio atau lokal — cek provider Indonesia kalau mau lebih murah/relevan), endpoint yang verifikasi nomor terdaftar ke user mana, dan format pesan yang bisa di-parse jadi lat/lng (atau minimal "saya aman, posisi kira-kira di X").
- [ ] **Offline-first sync** — `checkpoint.id` client-suppliable udah disiapin dari Task 1 buat ini. Yang belum: desain queue di App (antrian checkpoint/post yang dibuat offline), strategi retry, dan resolusi konflik kalau dua device sync checkpoint yang tumpang tindih.
- [ ] **Push notification** — notify follower pas ada checkpoint/post baru dipublish. Butuh FCM (Android)/APNs (iOS), tabel buat nyimpen device token per user, dan trigger di `create_checkpoint`/`create_post` (mirip pola `audit::log` yang udah ada — best-effort, gak boleh gagalin request utama).
- [ ] **Geofencing/proximity** — alert kalau ada bikepacker lain di radius tertentu, atau validasi jarak wajar antar checkpoint berurutan (deteksi anomali GPS). Kemungkinan butuh extension PostGIS di Postgres kalau perhitungan geospasialnya makin kompleks dari sekadar Haversine manual.

## Di luar 3 task ini (dicatat, belum di-task-in)

- **Report/Moderasi** — belum diimplementasikan sama sekali (juga belum ada buat User). `journey_equipment`/`journey_sponsors` di atas udah nyiapin kolom `status` + rencana `reports.target_type`, tapi sistem report-nya sendiri nunggu dibangun terpisah.
- **Shared journey, shared post lintas journey, fitur sponsor matching/pembayaran, `track_segments` (GPX)** — sengaja di luar scope, lihat `SYSTEM_DESIGN.md` bagian "Di luar scope tahap pertama".
