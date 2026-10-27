-- Additive account preferences; 0007 is reserved for the separate anime branch.
CREATE TABLE user_preferences (
    user_id TEXT PRIMARY KEY NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    values_json TEXT NOT NULL CHECK(json_valid(values_json))
);
