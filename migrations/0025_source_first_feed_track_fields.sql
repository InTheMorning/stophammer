ALTER TABLE feeds ADD COLUMN publisher TEXT;
ALTER TABLE feeds ADD COLUMN release_artist TEXT;
ALTER TABLE feeds ADD COLUMN release_artist_sort TEXT;
ALTER TABLE feeds ADD COLUMN release_date INTEGER;
ALTER TABLE feeds ADD COLUMN release_kind TEXT;

ALTER TABLE tracks ADD COLUMN image_url TEXT;
ALTER TABLE tracks ADD COLUMN language TEXT;
ALTER TABLE tracks ADD COLUMN track_artist TEXT;
ALTER TABLE tracks ADD COLUMN track_artist_sort TEXT;
