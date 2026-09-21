-- Encodes the "draft journeys hide their checkpoints/posts too" rule once,
-- here, instead of relying on every future query to remember the join.
-- Plain views (not materialized) so they're always consistent, never stale.
CREATE VIEW visible_checkpoints AS
SELECT c.* FROM checkpoints c
JOIN journeys j ON j.id = c.journey_id
WHERE c.status = 'published' AND j.status != 'draft';

CREATE VIEW visible_posts AS
SELECT p.* FROM posts p
JOIN checkpoints c ON c.id = p.checkpoint_id
JOIN journeys j ON j.id = c.journey_id
WHERE p.status = 'published' AND c.status = 'published' AND j.status != 'draft';
