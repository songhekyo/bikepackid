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
  start_lat, start_lng, end_lat, end_lng (nullable)

track_segments        -- opsional, dari upload file GPX
  id, journey_id, geojson_linestring, source ('gpx_upload'), uploaded_at

checkpoints            -- titik lokasi manual-trigger
  id, journey_id, lat, lng, captured_at, title
  trigger_type ('manual' | 'retroactive')
  status ('published' | 'flagged' | 'removed')

posts                  -- konten nempel ke checkpoint
  id, checkpoint_id, type ('video' | 'text' | 'thread_item'),
  body, media_url, parent_post_id (nullable, buat thread), created_at
  status ('published' | 'flagged' | 'removed')
```

Alur pengisian lokasi: **manual trigger** (tap "Tambah Titik" → HP ambil GPS lewat Geolocation API sekali saat itu) atau **retroaktif** (drop pin di peta / cari nama tempat via geocoding). User tidak pernah input angka lat/long langsung. GPX upload independen dari checkpoint — cuma buat gambar garis rute penuh di peta.

`journeys.start_lat/start_lng` dan `end_lat/end_lng` — titik awal & akhir rencana rute, diisi lewat cara yang sama (drop pin / cari nama tempat via geocoding, bukan input angka manual), konsisten sama prinsip di atas. Bedanya sama `checkpoints`: dua kolom ini cuma nunjukin **titik ujung rencana** (berguna khusus buat journey yang masih `planning` — belum ada checkpoint sama sekali karena trip belum mulai, jadi ini satu-satunya info lokasi yang bisa ditampilin di peta buat pitch sponsor), bukan titik-titik yang dilewatin selama perjalanan (itu tugas `checkpoints`). Kolomnya **nullable** di database (biar journey `draft` yang masih ditulis/belum lengkap tetap bisa disimpan), tapi **wajib** begitu keluar dari `draft` — ditegakkan lewat `CHECK` constraint di database (`status = 'draft' OR (start_lat IS NOT NULL AND start_lng IS NOT NULL AND end_lat IS NOT NULL AND end_lng IS NOT NULL)`), bukan cuma validasi di kode aplikasi. Ini sengaja di level database supaya invariant-nya terjamin walau ada jalur lain yang nyentuh tabel ini nanti (endpoint baru, script backfill, dll) — bukan bergantung ke tiap developer inget nulis pengecekan yang sama berulang-ulang.

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

### Journey Equipment (didesain di sini, implementasi nyusul PR terpisah)

Bikepacker sering mau nunjukin gear yang dipakai di satu journey — bukan cuma sepeda, tapi juga kamera, helm, tenda, dll. Awalnya dipikir sebagai "bikecheck" (khusus sepeda), tapi digeneralisasi jadi satu konsep equipment/gear yang fleksibel, karena tiap item — apapun kategorinya — bakal jadi titik koneksi yang sama ke commerce nanti (Fase 4): orang liat journey, liat gear apa yang dipakai, klik buat beli barang yang sama.

```
equipment_categories      -- lookup table, bukan enum
  id, name (unique)        -- 'Sepeda', 'Ban', 'Groupset', 'Tas', 'Kamera',
                            -- 'Helm', 'Tenda', 'Kompor', dst — di-seed lewat
                            -- migration, admin bisa nambah baris baru kapan
                            -- aja tanpa migration lagi

journey_equipment
  id, journey_id, category_id, name, brand (nullable),
  product_url (nullable), notes, created_at
  status ('published' | 'flagged' | 'removed')
```

- **`category_id`** — FK ke `equipment_categories`, **bukan teks bebas**. Alasan pakai lookup table dan bukan Postgres `ENUM`: taksonomi gear bikepacking terus nambah (kategori baru kayak "power bank"/"GPS device" bisa muncul kapan aja), dan nambah value ke `ENUM` butuh migration setiap kali — nambah baris ke lookup table enggak. FK juga nyegah typo/duplikat ('Bike' vs 'bike' vs 'bicycle') yang bisa kejadian kalau teks bebas.
- **`brand`** — opsional, teks bebas. Berguna buat matching ke produk beneran nanti.
- **`product_url`** — opsional, link luar (misal link affiliate/toko tempat beli). Ini seam murah ke commerce **sekarang**, bukan bangun infrastruktur marketplace beneran — begitu ada tabel `products` sungguhan di Fase 4, tinggal nambah kolom `product_id` (nullable FK) di sampingnya, additive lagi. Karena ini link luar dari user (bebas isi apa aja, termasuk berpotensi disalahgunakan buat link spam/phishing), `journey_equipment` ikut ke-cover sistem Report/Moderasi di bawah (`status` + `target_type = 'equipment'`) — bukan satu-satunya user-generated content yang gak ada jalur moderasinya.
- Satu journey bisa punya 0+ equipment — semuanya opsional, gak ada yang wajib diisi.
- Permission-nya sama kayak checkpoint/post: cuma pemilik journey yang bisa nambah/ubah, gak ada isu kepemilikan bersama kayak 3 item "di luar scope" di atas — makanya ini didesain sekarang walau implementasinya nyusul PR terpisah setelah loop inti (journey→checkpoint→post) kebukti jalan.

### Report / Moderasi (belum diimplementasikan)

Kebijakan: **konten langsung tayang saat diposting** (`published`), moderator cuma bertindak kalau ada laporan — bukan approval-first. Cocok buat komunitas yang masih kecil/awal, tidak butuh moderator standby 24/7.

```
reports
  id, reporter_user_id, target_type ('journey'|'checkpoint'|'post'|'equipment'|'user'),
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
