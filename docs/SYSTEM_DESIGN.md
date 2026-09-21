# bikepackid — Desain Sistem

Portal media + marketplace untuk komunitas bikepacker Indonesia. Dokumen ini merangkum keputusan desain sistem yang sudah diambil, supaya tidak hilang di percakapan dan bisa jadi acuan pengembangan lanjutan.

Status implementasi saat ini: **sistem User (login Google OAuth + role) sudah dibangun** di `backend/` (Rust/Axum/Postgres). Entity lain di bawah ini (Journey, Checkpoint, Post, Report, Marketplace) **belum diimplementasikan** — statusnya rencana/desain.

## Roadmap

1. **Riset & validasi** — wawancara komunitas bikepacker existing, tentukan niche awal.
2. **MVP Media/Konten** — sistem Journey + Checkpoint + Post (rute, cerita per titik). Bangun audiens dulu, SEO organik.
3. **Komunitas** — interaksi (komentar, forum), notifikasi/digest.
4. **Marketplace** — baru masuk fase jualan setelah ada audiens (Fase 2 selesai). Mulai ringan (affiliate/listing) sebelum toko sendiri penuh.
5. **Skala & Optimasi** — personalisasi, membership, analytics.

Urutan intinya: **konten dulu → komunitas → transaksi.**

## Platform

- **Web** (publik) — media, artikel, browsing journey, marketplace. Tanpa install, SEO-friendly. Diputuskan belakangan (framework belum dipilih).
- **App** (khusus login, `creator` role ke atas) — satu aplikasi buat kreator (log journey, trigger checkpoint, posting konten) *dan* tim admin (moderasi, kelola user, kelola marketplace), dibedakan lewat role, bukan app terpisah. Rekomendasi: cross-platform (React Native/Flutter), bukan native iOS+Android terpisah.

## Entity & Data Model

### User (✅ diimplementasikan)

```
users
  id            uuid
  google_id     text (unique)   -- login cuma via Google, tanpa password
  email         text (unique)
  name          text
  avatar_url    text (nullable)
  role          user_role enum
  created_at    timestamptz
```

**Role** (`viewer` / `creator` / `moderator` / `admin` / `superadmin`):

| Aksi | viewer | creator | moderator | admin | superadmin |
|---|---|---|---|---|---|
| Browse, follow, komentar, belanja | ✓ | ✓ | ✓ | ✓ | ✓ |
| Login ke **app** | ✗ | ✓ | ✓ | ✓ | ✓ |
| Bikin Journey/Checkpoint/Post | ✗ | ✓ (milik sendiri) | ✓ | ✓ | ✓ |
| Moderasi konten orang lain | ✗ | ✗ | ✓ | ✓ | ✓ |
| Kelola user (ban/verify) | ✗ | ✗ | ✓ | ✓ | ✓ |
| Kelola marketplace | ✗ | ✗ | ✗ | ✓ | ✓ |
| Kelola role user lain | ✗ | ✗ | ✗ | ✗ | ✓ |

Default role saat daftar via web: `viewer`.

### Journey, TrackSegment, Checkpoint, Post (belum diimplementasikan)

```
journeys
  id, user_id, title, description, status, start_date, end_date, cover_image
  status ('draft' | 'planning' | 'published' | 'archived')

track_segments        -- opsional, dari upload file GPX
  id, journey_id, geojson_linestring, source ('gpx_upload'), uploaded_at

checkpoints            -- titik lokasi manual-trigger
  id, journey_id, lat, lng, captured_at, title, trigger_type ('manual')
  status ('published' | 'flagged' | 'removed')

posts                  -- konten nempel ke checkpoint
  id, checkpoint_id, type ('video' | 'text' | 'thread_item'),
  body, media_url, parent_post_id (nullable, buat thread), created_at
  status ('published' | 'flagged' | 'removed')
```

Alur pengisian lokasi: **manual trigger** (tap "Tambah Titik" → HP ambil GPS lewat Geolocation API sekali saat itu) atau **retroaktif** (drop pin di peta / cari nama tempat via geocoding). User tidak pernah input angka lat/long langsung. GPX upload independen dari checkpoint — cuma buat gambar garis rute penuh di peta.

Video di-**embed** dari YouTube/Instagram/TikTok (bukan hosting sendiri) — hemat biaya storage/bandwidth.

**Status journey** (beda dari checkpoint/post yang langsung `published` saat dibuat — lihat kebijakan moderasi di bawah):
- `draft` — privat, cuma pemilik yang bisa lihat.
- `planning` — publik, tapi trip-nya belum mulai. Buat bikepacker yang mau share rencana rute dan **cari sponsor** sebelum berangkat — deskripsi journey (field `description`) yang jadi tempat pitch-nya, bukan fitur sponsor terpisah (belum didesain, lihat "Di luar scope" di bawah).
- `published` — publik, trip lagi jalan/udah selesai, checkpoint terus ditambah.
- `archived` — publik, udah gak aktif lagi.

Aturan visibility yang penting: bedanya cuma `draft` vs selain-`draft` — `planning`/`published`/`archived` semua publik, bedanya cuma gimana ditampilin di frontend nanti (badge "planning" vs trip yang lagi live), bukan soal siapa yang boleh lihat. Konsekuensinya: checkpoint/post yang statusnya sendiri udah `published` **tetap gak kelihatan publik** kalau journey induknya masih `draft` — jadi cek visibility checkpoint/post harus ikut cek status journey induknya, gak cukup cek status miliknya sendiri doang.

**Batasan desain (Monolith First)**: Journey + Checkpoint + Post dibangun sebagai satu modul di dalam backend Rust yang udah ada (bukan service terpisah) — boundary-nya jelas (tabel sendiri, diakses cuma lewat fungsi modul itu) supaya bisa diekstrak nanti kalau beneran perlu, tapi gak bayar cost distributed system (auth propagation lintas service, dll) selama belum ada alasan konkret buat mecah. `track_segments` (upload GPX) sengaja di luar scope tahap pertama — butuh dependency baru (parsing GPX, object storage) yang belum ada di codebase.

**Di luar scope tahap pertama** (dicatat di sini biar gak hilang dari diskusi, tapi sengaja ditunda sampai ada kebutuhan nyata):
- **Shared journey** — dua bikepacker yang jalan bareng dan mau journey-nya dimiliki bersama (bukan cuma satu `user_id`). Butuh tabel kolaborator + alur invite + keputusan soal siapa boleh apa — kompleksitas produk yang lebih besar dari sekadar skema data.
- **Shared post lintas journey** — dua bikepacker yang jalan **terpisah** (journey masing-masing, mulai dari tempat beda), ketemu di satu titik, dan mau pakai post yang sama di titik itu, lalu pisah lagi. Beda dari shared journey — ini butuh relasi many-to-many antara post dan checkpoint (satu post bisa nempel di checkpoint lebih dari satu journey), bukan kepemilikan bersama satu journey.
- **Fitur sponsor itu sendiri** (entity sponsor, form kontak/inquiry, pembayaran) — status `planning` di atas cuma bikin journey kelihatan publik lebih awal, gak termasuk tooling buat sponsor beneran connect ke bikepacker-nya.

Ketiganya dirancang supaya **aditif** kalau nanti dibangun — gak butuh migrasi yang ngubah/hapus kolom yang udah ada, cuma nambah tabel baru.

### Report / Moderasi (belum diimplementasikan)

Kebijakan: **konten langsung tayang saat diposting** (`published`), moderator cuma bertindak kalau ada laporan — bukan approval-first. Cocok buat komunitas yang masih kecil/awal, tidak butuh moderator standby 24/7.

```
reports
  id, reporter_user_id, target_type ('journey'|'checkpoint'|'post'|'user'),
  target_id, reason, status ('open'|'dismissed'|'upheld'),
  reviewed_by, reviewed_at, created_at
```

Alur: user lapor → `Report` (status `open`) + konten jadi `flagged` (tetap tayang) → moderator putuskan **dismiss** (konten balik `published`) atau **uphold** (konten `removed`).

### Marketplace (belum didesain detail)

Entity yang perlu dirancang saat masuk Fase 4 roadmap: `Product`, `Order`, `OrderItem`, inventori/stok. Belum ada keputusan soal payment gateway (kandidat: Midtrans/Xendit untuk pasar Indonesia).

## Auth & Keamanan (✅ diimplementasikan, lihat `backend/README.md`)

Login Google OAuth only (PKCE + CSRF state), session JWT di cookie httpOnly + `Secure`, dibackup tabel `sessions` di DB supaya bisa di-revoke (logout/ban beneran mencabut akses, bukan cuma hapus cookie). Event login/logout tercatat di `audit_logs`.

## Biaya & Infra

Strategi: pilih stack dengan free tier generous supaya biaya bulanan awal ~Rp0, cuma **domain** yang pasti keluar duit dari hari 1.

| Kebutuhan | Opsi (free tier dulu) | Perlu hati-hati |
|---|---|---|
| Map | Mapbox (~50rb load/bulan gratis) | - |
| Geocoding | Mapbox Geocoding / Nominatim | Google Places (free credit cepat habis) |
| Database | Postgres (Supabase/Neon) | - |
| File storage (GPX, foto) | Cloudflare R2 / Supabase Storage | - |
| Auth | Google OAuth (sendiri, sudah dibangun) | - |
| Hosting | Vercel/Netlify (frontend), Railway/Render/Fly.io (backend) | - |
| Video | Embed YouTube/IG/TikTok (gratis total) | - |

Kalau app native jadi dipublish ke store: **Apple Developer Program** $99/tahun, **Google Play Console** $25 sekali bayar (bisa ditunda pakai TestFlight/internal testing dulu).

Google OAuth consent screen: mode default "Testing" (~100 user), perlu submit verifikasi Google (gratis, ada proses review) sebelum publik ke user umum.
