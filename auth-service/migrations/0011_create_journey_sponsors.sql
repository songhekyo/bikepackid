CREATE TYPE sponsor_status AS ENUM ('published', 'flagged', 'removed');

CREATE TABLE journey_sponsors (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    journey_id  UUID NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    logo_url    TEXT,
    website_url TEXT,
    notes       TEXT,
    status      sponsor_status NOT NULL DEFAULT 'published',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX journey_sponsors_journey_id_idx ON journey_sponsors (journey_id);

-- Same draft-journey-hides-children rule as visible_checkpoints/visible_posts
-- (0008) and visible_equipment (0010). Kept in this migration rather than a
-- dedicated one, for the same reason 0010 gave: just one new table here, no
-- multi-table bundle to justify splitting the view out separately.
CREATE VIEW visible_sponsors AS
SELECT s.* FROM journey_sponsors s
JOIN journeys j ON j.id = s.journey_id
WHERE s.status = 'published' AND j.status != 'draft';
