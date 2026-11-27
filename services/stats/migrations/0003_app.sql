-- What the Quiet.md app reports once a day (src/usage.rs): per UTC day and random
-- install id, how often it was opened (launched or brought to the front, at most
-- once per half hour). No IP, no library content. The cron turns it into app.* metrics.
-- A day reported again (a resend) replaces the earlier report.
CREATE TABLE app_reports (
  day TEXT NOT NULL,          -- YYYY-MM-DD, UTC, as the client counted it
  install TEXT NOT NULL,      -- random id the app made (uuid v4), not the CLI's
  version TEXT NOT NULL,      -- the app's version, e.g. 0.1.15
  os TEXT NOT NULL,           -- macos, windows, linux
  arch TEXT NOT NULL,         -- aarch64, x86_64
  opens INTEGER NOT NULL,
  received_at TEXT NOT NULL,
  PRIMARY KEY (day, install)
);
CREATE INDEX app_reports_install ON app_reports (install, day);
