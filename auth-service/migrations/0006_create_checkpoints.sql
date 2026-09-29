CREATE TYPE checkpoint_status AS ENUM ('published', 'flagged', 'removed');

CREATE TABLE checkpoints (
    id           UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    journey_id   UUID NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    lat          DOUBLE PRECISION NOT NULL,
    lng          DOUBLE PRECISION NOT NULL,
    captured_at  TIMESTAMPTZ NOT NULL,
    title        TEXT,
    -- 'manual' (tap "Tambah Titik", GPS grabbed there and then) or
    -- 'retroactive' (pin dropped on a map / place search after the fact).
    trigger_type TEXT NOT NULL DEFAULT 'manual',
    status       checkpoint_status NOT NULL DEFAULT 'published',
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX checkpoints_journey_id_idx ON checkpoints (journey_id);
