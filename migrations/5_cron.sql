create extension pg_cron;

select cron.schedule('0 0 * * *', $$vacuum$$);
select cron.schedule('0 0 * * *', $$delete from Sessions where expire_at < now()$$);