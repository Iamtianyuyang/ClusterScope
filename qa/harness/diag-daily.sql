-- QA diagnostic #2: does the SQL filter the server uses match rows, and does
-- the REST API return them? (daily / 90-day tier)
-- Run: /public/.../pg16/bin/psql "$PGURL" -f qa/harness/diag-daily.sql
\echo '--- all daily buckets ---'
SELECT node_id, metric_name, day_bucket, avg_value FROM metrics_daily ORDER BY day_bucket;
\echo '--- exact server filter, 30d ago -> 8d ago (bigint-safe) ---'
SELECT day_bucket, metric_name, avg_value
  FROM metrics_daily
 WHERE node_id = 'qa-node-01'
   AND day_bucket >= to_timestamp((extract(epoch from now())::bigint * 1000 - 30::bigint * 86400000) / 1000.0)
   AND day_bucket <= to_timestamp((extract(epoch from now())::bigint * 1000 - 8::bigint * 86400000) / 1000.0)
 ORDER BY day_bucket ASC;
\echo '--- same filter, date-vs-timestamp cast detail ---'
SELECT (now() - interval '20 days')::date AS bucket_20d,
       ((now() - interval '20 days')::date)::timestamptz AS bucket_20d_as_tstz,
       now() - interval '30 days' AS lower_bound,
       ((now() - interval '20 days')::date)::timestamptz >= now() - interval '30 days' AS passes_lower;
