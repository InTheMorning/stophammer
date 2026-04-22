CREATE INDEX IF NOT EXISTS idx_tracks_artist_lower ON tracks(lower(track_artist));
