CREATE TYPE equipment_status AS ENUM ('published', 'flagged', 'removed');

CREATE TABLE journey_equipment (
    id          UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    journey_id  UUID NOT NULL REFERENCES journeys(id) ON DELETE CASCADE,
    category_id UUID NOT NULL REFERENCES equipment_categories(id),
    name        TEXT NOT NULL,
    brand       TEXT,
    product_url TEXT,
    notes       TEXT,
    status      equipment_status NOT NULL DEFAULT 'published',
    created_at  TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX journey_equipment_journey_id_idx ON journey_equipment (journey_id);

-- Same draft-journey-hides-children rule as visible_checkpoints/visible_posts
-- (0008_create_visibility_views.sql). Kept in this migration rather than a
-- dedicated one: 0011 is already reserved for journey_sponsors (Task 3,
-- which runs in parallel with this task), and unlike 0008 — which bundled
-- views for three tables Task 1 shipped together — this task only adds one
-- new table, so there's no multi-table bundle to justify splitting it out.
CREATE VIEW visible_equipment AS
SELECT e.* FROM journey_equipment e
JOIN journeys j ON j.id = e.journey_id
WHERE e.status = 'published' AND j.status != 'draft';
