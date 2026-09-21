CREATE TYPE journey_status AS ENUM ('draft', 'planning', 'published', 'archived');

CREATE TABLE journeys (
    id              UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    user_id         UUID NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title           TEXT NOT NULL,
    description     TEXT,
    status          journey_status NOT NULL DEFAULT 'draft',
    start_date      DATE,
    end_date        DATE,
    cover_image     TEXT,
    start_lat       DOUBLE PRECISION,
    start_lng       DOUBLE PRECISION,
    end_lat         DOUBLE PRECISION,
    end_lng         DOUBLE PRECISION,
    seeking_sponsor BOOLEAN NOT NULL DEFAULT false,
    donation_url    TEXT,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    -- A journey can sit incomplete in `draft` with no endpoints yet, but
    -- once it's visible to anyone else (planning/published/archived) it
    -- must have somewhere to show on a map. Enforced here, not just in
    -- application code, so no future code path (new endpoint, backfill
    -- script) can slip a journey past this without a real database error.
    CONSTRAINT journeys_endpoints_required_outside_draft CHECK (
        status = 'draft' OR (
            start_lat IS NOT NULL AND start_lng IS NOT NULL AND
            end_lat IS NOT NULL AND end_lng IS NOT NULL
        )
    )
);
CREATE INDEX journeys_user_id_idx ON journeys (user_id);
