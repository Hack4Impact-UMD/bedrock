-- Postgres can't drop an enum value, so rebuild the type without it.
update courier.emails set status = 'failed' where status = 'unknown';
alter type courier.email_status rename to email_status_old;
create type courier.email_status as enum ('sent', 'failed');
alter table courier.emails
    alter column status type courier.email_status using status::text::courier.email_status;
drop type courier.email_status_old;
