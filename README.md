# bikepackid

Portal media + marketplace untuk komunitas bikepacker Indonesia — jurnal perjalanan (journey/checkpoint), konten, dan (belakangan) marketplace ringan. Detail lengkap desain sistem & roadmap ada di [`docs/SYSTEM_DESIGN.md`](docs/SYSTEM_DESIGN.md).

## Status

**Live**: [bikepacking.cyou](https://bikepacking.cyou)

Yang udah dibangun & production: sistem User (login Google OAuth, role-based access `viewer`/`creator`/`moderator`/`admin`/`superadmin`), observability (trace ke Grafana Cloud, uptime monitoring), dan infra deploy otomatis (CI build image → GHCR → auto-deploy ke VPS). Entity konten (Journey/Checkpoint/Post) dan marketplace masih di tahap desain — lihat roadmap di `docs/SYSTEM_DESIGN.md`.

## Struktur

- [`backend/`](backend/) — API Rust/Axum/Postgres. Mulai dari [`backend/README.md`](backend/README.md) buat jalanin lokal, struktur kode, dan endpoint yang udah ada.
- [`docs/SYSTEM_DESIGN.md`](docs/SYSTEM_DESIGN.md) — desain sistem: model data, roadmap fase (konten → komunitas → marketplace), strategi biaya/infra.
- [`docs/INFRA_HISTORY.md`](docs/INFRA_HISTORY.md) — riwayat keputusan infra dari nol (kenapa Nusa VPS, kenapa pindah ke Supabase, kenapa CI/CD-nya begini) — berguna kalau ada keputusan yang keliatan aneh tanpa konteks.

## Deploy & Infra

Panduan deploy lengkap ada di [`backend/DEPLOY.md`](backend/DEPLOY.md). Ringkasnya: GitHub Actions build & test tiap push, push image ke GHCR kalau masuk `main`, dan Watchtower di VPS yang narik image baru otomatis (gak perlu SSH manual tiap deploy, kecuali ada perubahan di `docker-compose.yml`/`deploy/config.alloy` sendiri).

Checklist yang masih perlu dibereskan sebelum benar-benar siap ke user umum ada di [`backend/TODO_PRODUCTION.md`](backend/TODO_PRODUCTION.md).
