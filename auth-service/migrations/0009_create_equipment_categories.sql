-- Lookup table, not an ENUM: equipment taxonomy grows over time (new
-- categories like "power bank"/"GPS device" can show up any time per
-- docs/SYSTEM_DESIGN.md), and adding a value to an ENUM needs a migration
-- every time — adding a row here doesn't. FK from journey_equipment also
-- prevents typos/duplicates ('Bike' vs 'bike' vs 'bicycle').
CREATE TABLE equipment_categories (
    id   UUID PRIMARY KEY DEFAULT gen_random_uuid(),
    name TEXT NOT NULL UNIQUE
);

INSERT INTO equipment_categories (name) VALUES
    ('Sepeda'), ('Ban'), ('Groupset'), ('Tas'),
    ('Kamera'), ('Helm'), ('Tenda'), ('Kompor');
