# bikepackid

Portal media + marketplace untuk komunitas bikepacker Indonesia — jurnal perjalanan (journey/checkpoint), konten, dan (belakangan) marketplace ringan. Detail lengkap desain sistem & roadmap ada di [`docs/SYSTEM_DESIGN.md`](docs/SYSTEM_DESIGN.md).

## Status

**Live**: [bikepacking.cyou](https://bikepacking.cyou)

Yang udah dibangun & production: sistem User (login Google OAuth, role-based access `viewer`/`creator`/`moderator`/`admin`/`superadmin`), observability (trace ke Grafana Cloud, uptime monitoring), dan infra deploy otomatis (CI build image → GHCR → auto-deploy ke server). Entity konten (Journey/Checkpoint/Post) dan marketplace masih di tahap desain — lihat roadmap di `docs/SYSTEM_DESIGN.md`.

Infra sekarang jalan di **AWS EC2** (sebelumnya VPS Nusa, cutover 28 September 2026) — lihat `docs/AWS_MIGRATION.md` dan `docs/INFRA_HISTORY.md` bagian 12.

## Struktur

- [`backend/`](backend/) — API Rust/Axum/Postgres. Mulai dari [`backend/README.md`](backend/README.md) buat jalanin lokal, struktur kode, dan endpoint yang udah ada.
- [`docs/SYSTEM_DESIGN.md`](docs/SYSTEM_DESIGN.md) — desain sistem: model data, roadmap fase (konten → komunitas → marketplace), strategi biaya/infra.
- [`docs/INFRA_HISTORY.md`](docs/INFRA_HISTORY.md) — riwayat keputusan infra dari nol (kenapa Nusa VPS awalnya, kenapa pindah ke Supabase lalu balik lagi ke Postgres self-hosted, kenapa akhirnya pindah ke AWS) — berguna kalau ada keputusan yang keliatan aneh tanpa konteks.
- [`docs/JOURNEY_TODO.md`](docs/JOURNEY_TODO.md) — breakdown implementasi Journey/Checkpoint/Post/Equipment/Sponsors, urut sesuai dependency.
- [`docs/AWS_MIGRATION.md`](docs/AWS_MIGRATION.md) — migrasi dari VPS ke AWS (selesai): alasan, keputusan yang diambil, dan cara nahan biaya tetap murah.

## Deploy & Infra

Panduan deploy aktif ada di [`backend/DEPLOY_AWS.md`](backend/DEPLOY_AWS.md) (AWS EC2, live sekarang). [`backend/DEPLOY.md`](backend/DEPLOY.md) (VPS Nusa) dibiarin ada sebagai referensi historis. Ringkasnya: GitHub Actions build & test tiap push, push image ke GHCR kalau masuk `main`, dan Watchtower di server yang narik image baru otomatis (gak perlu akses manual tiap deploy, kecuali ada perubahan di `docker-compose.yml`/`deploy/config.alloy` sendiri).

Checklist yang masih perlu dibereskan sebelum benar-benar siap ke user umum ada di [`backend/TODO_PRODUCTION.md`](backend/TODO_PRODUCTION.md).
