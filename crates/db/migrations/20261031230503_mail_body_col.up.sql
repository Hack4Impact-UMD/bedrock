-- Nullable: emails sent before this migration have no stored body.
alter table courier.emails add column body text;
