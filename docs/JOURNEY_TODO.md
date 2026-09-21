# Journey — implementation TODO

Breakdown kerja buat 3 task yang udah di-track (lihat sesi kerja, task #1-#3), urut sesuai dependency: task 2 dan 3 sama-sama nunggu task 1 kelar (butuh tabel `journeys` ada duluan), tapi 2 dan 3 sendiri gak saling gantung. Rujukan desain lengkap ada di [`SYSTEM_DESIGN.md`](./SYSTEM_DESIGN.md); rujukan implementasi detail (file per file) ada di plan file sesi ini.

## Task 1 — Journey/Checkpoint/Post core module

Loop inti: create journey → checkpoint → post. Semua yang lain nunggu ini kelar duluan.

### Migrasi
- [ ] `0005_create_journeys.sql` — tabel `journeys` (status enum 4 nilai: `draft`/`planning`/`published`/`archived`; `start_lat`/`start_lng`/`end_lat`/`end_lng` nullable; `seeking_sponsor` boolean default `false`; `donation_url` nullable; `CHECK` constraint: wajib ada start/end lat-lng begitu keluar dari `draft`)
- [ ] `0006_create_checkpoints.sql` — tabel `checkpoints` (status enum `published`/`flagged`/`removed`; `trigger_type` `manual`/`retroactive`)
- [ ] `0007_create_posts.sql` — tabel `posts` (`checkpoint_id` NOT NULL — keputusan final, gak ada post level-journey; status enum sama kayak checkpoint)
- [ ] `0008_create_visibility_views.sql` — `visible_checkpoints` dan `visible_posts` (VIEW, bukan materialized) — harus setelah 3 tabel di atas karena join ke semuanya

### Kode Rust
- [ ] `src/error.rs` — tambah varian `AppError::NotFound` → `StatusCode::NOT_FOUND`
- [ ] `src/journey/mod.rs` — struct `Journey`/`Checkpoint`/`Post` + enum status (`sqlx::FromRow`, `Serialize`, pola sama kayak `models/user.rs`)
- [ ] `src/journey/service.rs` — `create_journey`, `get_journey`, `list_public_journeys` (pagination `limit`/`offset`, clamp maks 50), `update_journey`, `create_checkpoint`, `list_checkpoints`, `create_post`, `list_posts`; ownership check (`user_id` cocok atau role `moderator`+) diisolasi di satu fungsi biar gampang di-extend pas ada collaborator nanti
- [ ] `src/routes/journey.rs` — handler tipis, pola sama kayak `routes/me.rs`; reads public (query ke `visible_checkpoints`/`visible_posts`, bukan tabel mentah), writes butuh `AuthUser` + `role.can_use_app()`
- [ ] `src/routes/mod.rs` — `pub mod journey;` + wire 4 route (`/journeys`, `/journeys/:id`, `/journeys/:id/checkpoints`, `/checkpoints/:id/posts`)
- [ ] `src/main.rs` — `mod journey;`

### Checkpoint ID (buat offline-sync nanti)
- [ ] `create_checkpoint` nerima `id` opsional dari client (bukan asumsi server yang selalu generate) — App (creator) bakal butuh ini pas offline-sync dibangun, jangan sampai jadi breaking change belakangan

### Test
- [ ] `test_support.rs` — `insert_journey(pool, user_id, status)`, `insert_checkpoint(pool, journey_id)`
- [ ] Unit test `journey/service.rs`: create+get roundtrip; `list_public_journeys` exclude draft, include planning/published/archived; checkpoint di journey draft gak nongol di list publik walau checkpoint-nya sendiri `published`; ownership check nolak non-owner non-moderator
- [ ] Router test `routes/mod.rs`: `POST /journeys` viewer→403, creator→201; `GET /journeys/:id` draft ke non-owner→404; `PATCH /journeys/:id` beda creator→403, moderator→200

### Docs & verifikasi
- [ ] `backend/README.md` — endpoint table + Struktur list (`src/journey/`)
- [ ] `cargo build` / `cargo test` / `cargo clippy --all-targets -- -D warnings` bersih
- [ ] Smoke test manual: draft→404 publik, `planning`→200+checkpoint/post ikut kelihatan
- [ ] Commit, push, PR

## Task 2 — Journey Equipment (nunggu Task 1)

Daftar gear (sepeda, kamera, helm, tenda, dll), informational, seam murah ke commerce.

### Migrasi
- [ ] `0009_create_equipment_categories.sql` — lookup table + seed data awal (Sepeda, Ban, Groupset, Tas, Kamera, Helm, Tenda, Kompor, dst)
- [ ] `0010_create_journey_equipment.sql` — `category_id` FK, `name`, `brand` nullable, `product_url` nullable, `notes` nullable, status enum (`published`/`flagged`/`removed`)

### Kode Rust
- [ ] Struct `Equipment`/`EquipmentCategory` (di `src/journey/mod.rs` atau file baru `src/journey/equipment.rs`)
- [ ] Service function: `create_equipment`, `list_equipment`, `list_equipment_categories`
- [ ] Route: `GET/POST /journeys/:id/equipment` — permission sama kayak checkpoint/post (cuma owner journey)

### Test & docs
- [ ] Unit + router test (pola sama kayak Task 1)
- [ ] `backend/README.md` update
- [ ] Commit, push, PR

## Task 3 — Journey Sponsors (nunggu Task 1, paralel sama Task 2)

Daftar sponsor yang udah deal (di luar platform), ditampilin di halaman journey.

### Migrasi
- [ ] `0011_create_journey_sponsors.sql` — `name`, `logo_url` nullable, `website_url` nullable, `notes` nullable, status enum

### Kode Rust
- [ ] Struct `Sponsor` (`src/journey/mod.rs` atau `src/journey/sponsor.rs`)
- [ ] Service function: `create_sponsor`, `list_sponsors`
- [ ] Route: `GET/POST /journeys/:id/sponsors` — permission sama kayak equipment

### Test & docs
- [ ] Unit + router test
- [ ] `backend/README.md` update
- [ ] Commit, push, PR

## Di luar 3 task ini (dicatat, belum di-task-in)

- **Report/Moderasi** — belum diimplementasikan sama sekali (juga belum ada buat User). `journey_equipment`/`journey_sponsors` di atas udah nyiapin kolom `status` + rencana `reports.target_type`, tapi sistem report-nya sendiri nunggu dibangun terpisah.
- **Shared journey, shared post lintas journey, fitur sponsor matching/pembayaran, `track_segments` (GPX)** — sengaja di luar scope, lihat `SYSTEM_DESIGN.md` bagian "Di luar scope tahap pertama".
