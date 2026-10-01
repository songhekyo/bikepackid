# bikepackid — Desain Sistem

Portal media + marketplace untuk komunitas bikepacker Indonesia. Dokumen ini merangkum keputusan desain sistem yang sudah diambil, supaya tidak hilang di percakapan dan bisa jadi acuan pengembangan lanjutan.

Status implementasi saat ini: **sistem User** (login Google OAuth + role), **Journey/Checkpoint/Post inti** (termasuk upload foto ke R2), dan **Journey Equipment** **sudah dibangun**, di dua service terpisah dalam monorepo Rust/Axum/Postgres yang sama: `auth-service/` (User, dulu namanya `backend/`) dan `journey-service/` (Journey/Checkpoint/Post/Equipment — diekstrak keluar dari `auth-service` belakangan, lihat `docs/AWS_MIGRATION.md` Fase 2). Entity lain di bawah ini (TrackSegment, Journey Sponsors, Report, Marketplace) **belum diimplementasikan** — statusnya rencana/desain.

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

### Journey, Checkpoint, Post (✅ diimplementasikan), TrackSegment (belum)

```
journeys
  id, user_id, title, description, status, start_date, end_date, cover_image
  status ('draft' | 'planning' | 'published' | 'archived')
  start_lat, start_lng, end_lat, end_lng (nullable)
  seeking_sponsor boolean (default false)
  donation_url text (nullable)

track_segments        -- opsional, dari upload file GPX
  id, journey_id, geojson_linestring, source ('gpx_upload'), uploaded_at

checkpoints            -- titik lokasi manual-trigger
  id, journey_id, lat, lng, captured_at, title
  trigger_type ('manual' | 'retroactive')
  status ('published' | 'flagged' | 'removed')

posts                  -- konten nempel ke checkpoint
  id, checkpoint_id, type ('photo' | 'video' | 'text' | 'thread_item'),
  body, media_url, parent_post_id (nullable, buat thread), created_at
  status ('published' | 'flagged' | 'removed')
```

Alur pengisian lokasi: **manual trigger** (tap "Tambah Titik" → HP ambil GPS lewat Geolocation API sekali saat itu) atau **retroaktif** (drop pin di peta / cari nama tempat via geocoding). User tidak pernah input angka lat/long langsung. GPX upload independen dari checkpoint — cuma buat gambar garis rute penuh di peta.

**Keputusan yang ditinjau ulang**: sempat dipertimbangkan bikin `posts.checkpoint_id` nullable + nambah `posts.journey_id`, biar ada "post level-journey" buat cerita yang bukan tentang satu titik spesifik (misal rangkuman akhir trip). Diputuskan **tetap** `checkpoint_id` wajib — dicek dulu ke aplikasi sejenis ([Pebbls](https://www.pebbls.com/how-to-track-your-bikepacking-adventure-and-tell-your-story/), yang model kontennya paling deket sama Journey/Checkpoint/Post di sini), dan mereka juga bikin setiap unit cerita ("Pebbl") selalu nempel ke lokasi, gak ada konsep "moment tanpa lokasi" terpisah. Kebutuhan cerita umum (bukan tentang satu titik) udah ke-cover sama `journeys.description` yang emang bebas teks. Keputusan ini juga sengaja diambil sebelum ada data post beneran — mengubahnya nanti butuh migrasi backfill yang lebih mahal daripada nambah tabel baru, jadi lebih murah diputuskan sekarang daripada nanti.

`journeys.start_lat/start_lng` dan `end_lat/end_lng` — titik awal & akhir rencana rute, diisi lewat cara yang sama (drop pin / cari nama tempat via geocoding, bukan input angka manual), konsisten sama prinsip di atas. Bedanya sama `checkpoints`: dua kolom ini cuma nunjukin **titik ujung rencana** (berguna khusus buat journey yang masih `planning` — belum ada checkpoint sama sekali karena trip belum mulai, jadi ini satu-satunya info lokasi yang bisa ditampilin di peta buat pitch sponsor), bukan titik-titik yang dilewatin selama perjalanan (itu tugas `checkpoints`). Kolomnya **nullable** di database (biar journey `draft` yang masih ditulis/belum lengkap tetap bisa disimpan), tapi **wajib** begitu keluar dari `draft` — ditegakkan lewat `CHECK` constraint di database (`status = 'draft' OR (start_lat IS NOT NULL AND start_lng IS NOT NULL AND end_lat IS NOT NULL AND end_lng IS NOT NULL)`), bukan cuma validasi di kode aplikasi. Ini sengaja di level database supaya invariant-nya terjamin walau ada jalur lain yang nyentuh tabel ini nanti (endpoint baru, script backfill, dll) — bukan bergantung ke tiap developer inget nulis pengecekan yang sama berulang-ulang.

Video di-**embed** dari YouTube/Instagram/TikTok (bukan hosting sendiri) — hemat biaya storage/bandwidth.

**Foto beda kasus dari video — di-hosting sendiri, bukan embed.** Dicek dulu ke aplikasi sejenis ([Pebbls](https://www.pebbls.com/), [Rolling Around](https://rollingaround.app/)) — keduanya upload foto beneran sebagai bagian inti dari tiap titik/moment, bukan link-out. Buat platform cerita perjalanan, foto jauh lebih sering dipakai daripada video, jadi maksa "paste link foto yang di-hosting di tempat lain" bakal jadi friksi berat di loop inti (beda dari video yang emang wajar berasal dari platform lain).

- **Storage: Cloudflare R2**, bukan Supabase Storage — R2 **gak ada biaya egress**, penting buat media-heavy public site (foto dilihat berkali-kali oleh banyak viewer). Free tier 10GB. Supabase Storage free tier lebih kecil dan bandwidth-nya berpotensi kena biaya begitu traffic naik.
- **Pola upload: presigned URL**, bukan proxy lewat backend. Backend generate URL upload yang udah di-sign, client (App) upload **langsung** ke R2 pakai URL itu — file gak pernah lewat VPS 1GB kita. Ini konsisten sama concern resource VPS yang jadi tema infra sepanjang project ini (lihat `INFRA_HISTORY.md`) — proxy file besar (foto bisa beberapa MB) lewat backend kecil itu buang-buang RAM/bandwidth yang gak perlu.
- `post.type` nambah `'photo'` — sebelumnya cuma `video`/`text`/`thread_item`.
- Setup R2 (bucket, API token/credential) adalah **prasyarat infra**, dilakuin sebelum implementasi Task 1 (Journey/Checkpoint/Post) mulai — lihat `docs/JOURNEY_TODO.md`.

**Status journey** (beda dari checkpoint/post yang langsung `published` saat dibuat — lihat kebijakan moderasi di bawah):
- `draft` — privat, cuma pemilik yang bisa lihat.
- `planning` — publik, tapi trip-nya belum mulai. Buat bikepacker yang mau share rencana rute dan **cari sponsor** sebelum berangkat — deskripsi journey (field `description`) yang jadi tempat pitch-nya, bukan fitur sponsor terpisah (belum didesain, lihat "Di luar scope" di bawah). `journeys.seeking_sponsor` (boolean, default `false`) — flag sederhana buat nandain "masih nyari sponsor", independen dari status: journey `planning` belum tentu masih nyari sponsor (misal udah dapet, tinggal nunggu berangkat), dan gak harus `planning` juga buat nyalain flag ini (trip yang udah `published` bisa aja masih buka slot sponsor tambahan). Bukan sistem matching/inquiry — sekadar tag yang bisa di-filter nanti begitu ada halaman "browse journey yang lagi cari sponsor" (belum dibangun sekarang, tapi kolomnya murah buat disiapin dari awal daripada migrasi tambahan nanti). `journeys.donation_url` (teks bebas, nullable) — link keluar ke platform donasi yang udah handle pembayaran sendiri (Saweria/Trakteer/Ko-fi/dst, bikepacker pilih sendiri). Bukan bikin payment processing sendiri — gak ada alasan duplikasi yang udah diselesein dengan baik sama platform yang udah establish. Sama kayak `product_url`/`website_url` di tempat lain: link luar apa adanya, gak divalidasi formatnya, backend gak pernah pegang duit sama sekali.
- `published` — publik, trip lagi jalan/udah selesai, checkpoint terus ditambah.
- `archived` — publik, udah gak aktif lagi.

Aturan visibility yang penting: bedanya cuma `draft` vs selain-`draft` — `planning`/`published`/`archived` semua publik, bedanya cuma gimana ditampilin di frontend nanti (badge "planning" vs trip yang lagi live), bukan soal siapa yang boleh lihat. Konsekuensinya: checkpoint/post yang statusnya sendiri udah `published` **tetap gak kelihatan publik** kalau journey induknya masih `draft` — jadi cek visibility checkpoint/post harus ikut cek status journey induknya, gak cukup cek status miliknya sendiri doang. Post juga ikut ke-hide kalau **checkpoint**-nya (bukan cuma journey-nya) lagi `flagged`/`removed` — 3 tingkat yang harus konsisten (journey → checkpoint → post), bukan 2.

Supaya aturan ini gak bergantung ke tiap developer inget nulis join yang bener tiap kali nulis query baru (gampang lupa, terutama nambah fitur baru di masa depan kayak "activity feed" yang query checkpoint/post langsung), aturannya ditegakkan lewat **Postgres VIEW**, bukan cuma konvensi kode aplikasi:

```sql
CREATE VIEW visible_checkpoints AS
SELECT c.* FROM checkpoints c
JOIN journeys j ON j.id = c.journey_id
WHERE c.status = 'published' AND j.status != 'draft';

CREATE VIEW visible_posts AS
SELECT p.* FROM posts p
JOIN checkpoints c ON c.id = p.checkpoint_id
JOIN journeys j ON j.id = c.journey_id
WHERE p.status = 'published' AND c.status = 'published' AND j.status != 'draft';
```

Kode aplikasi yang butuh checkpoint/post publik query ke view ini, bukan ke tabel mentahnya — jadi query yang bener itu sama gampangnya (bukan lebih ribet) dibanding query yang salah, gak ada alasan buat "males join" dan langsung query tabel aslinya. View ini gak nyimpen data sendiri (bukan materialized view), jadi selalu konsisten real-time, gak ada risiko data basi kayak kalau dipakai pendekatan denormalisasi/cache.

**Pagination**: `GET /journeys` (feed publik lintas semua user, bakal terus nambah) pakai `?limit=&offset=` — `limit` di-clamp di handler (default 20, maks 50), response array polos tanpa `total_count`/`has_more` (client tau abis kalau baliknya lebih sedikit dari `limit`; nge-`COUNT(*)` tiap request buat metadata itu mahal buat manfaat yang kecil di tahap ini). `GET /journeys/:id/checkpoints` dan `GET /checkpoints/:id/posts` belum butuh pagination — scoped ke satu journey/checkpoint, ukurannya natural terbatas buat MVP. Kalau offset-based mulai kerasa masalahnya (drift pas ada insert baru di tengah pagination), upgrade ke cursor-based itu ganti di satu endpoint doang, gak butuh migrasi data.

**Batasan desain (Monolith First)**: Journey + Checkpoint + Post dibangun sebagai satu modul di dalam backend Rust yang udah ada (bukan service terpisah) — boundary-nya jelas (tabel sendiri, diakses cuma lewat fungsi modul itu) supaya bisa diekstrak nanti kalau beneran perlu, tapi gak bayar cost distributed system (auth propagation lintas service, dll) selama belum ada alasan konkret buat mecah. `track_segments` (upload GPX) sengaja di luar scope tahap pertama — butuh dependency baru (parsing GPX, object storage) yang belum ada di codebase.

**Di luar scope tahap pertama** (dicatat di sini biar gak hilang dari diskusi, tapi sengaja ditunda sampai ada kebutuhan nyata):
- **Shared journey** — dua bikepacker yang jalan bareng dan mau journey-nya dimiliki bersama (bukan cuma satu `user_id`). Butuh tabel kolaborator + alur invite + keputusan soal siapa boleh apa — kompleksitas produk yang lebih besar dari sekadar skema data.
- **Shared post lintas journey** — dua bikepacker yang jalan **terpisah** (journey masing-masing, mulai dari tempat beda), ketemu di satu titik, dan mau pakai post yang sama di titik itu, lalu pisah lagi. Beda dari shared journey — ini butuh relasi many-to-many antara post dan checkpoint (satu post bisa nempel di checkpoint lebih dari satu journey), bukan kepemilikan bersama satu journey.
- **Fitur sponsor itu sendiri** (entity sponsor, form kontak/inquiry, pembayaran) — status `planning` di atas cuma bikin journey kelihatan publik lebih awal, gak termasuk tooling buat sponsor beneran connect ke bikepacker-nya.

Ketiganya dirancang supaya **aditif** kalau nanti dibangun — gak butuh migrasi yang ngubah/hapus kolom yang udah ada, cuma nambah tabel baru.

### Journey Equipment (sudah diimplementasikan, lihat `journey-service/`)

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

### Journey Sponsors (didesain di sini, implementasi nyusul PR terpisah)

Beda dari `seeking_sponsor` di atas (nandain "masih nyari") — ini buat nunjukin sponsor yang **udah deal**, ditampilin di halaman journey (misal "Didukung oleh: ..."). Sama kayak `journey_equipment`: murni informational, gak ada logic matching/inquiry/pembayaran (itu tetep bagian dari "fitur sponsor itu sendiri" yang di luar scope), jadi kompleksitasnya setara — didesain sekarang, implementasi nyusul bareng `journey_equipment`.

```
journey_sponsors
  id, journey_id, name, logo_url (nullable), website_url (nullable),
  notes (nullable), created_at
  status ('published' | 'flagged' | 'removed')
```

- Kesepakatan sponsor-nya sendiri terjadi **di luar platform** (DM, email, dst) — tabel ini cuma catetan buat ditampilin, bukan tempat nego/kontrak.
- `status` + masuk ke `reports.target_type` (`'sponsor'`) sama kayak `journey_equipment` — alasan sama: ada link luar (`website_url`) dari user, perlu jalur moderasi.
- Satu journey bisa punya 0+ sponsor, semuanya opsional.

### Report / Moderasi (belum diimplementasikan)

Kebijakan: **konten langsung tayang saat diposting** (`published`), moderator cuma bertindak kalau ada laporan — bukan approval-first. Cocok buat komunitas yang masih kecil/awal, tidak butuh moderator standby 24/7.

```
reports
  id, reporter_user_id, target_type ('journey'|'checkpoint'|'post'|'equipment'|'sponsor'|'user'),
  target_id, reason, status ('open'|'dismissed'|'upheld'),
  reviewed_by, reviewed_at, created_at
```

Alur: user lapor → `Report` (status `open`) + konten jadi `flagged` (tetap tayang) → moderator putuskan **dismiss** (konten balik `published`) atau **uphold** (konten `removed`).

### Marketplace (belum didesain detail)

Entity yang perlu dirancang saat masuk Fase 4 roadmap: `Product`, `Order`, `OrderItem`, inventori/stok. Belum ada keputusan soal payment gateway (kandidat: Midtrans/Xendit untuk pasar Indonesia).

## Auth & Keamanan (✅ diimplementasikan, lihat `auth-service/README.md`)

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
