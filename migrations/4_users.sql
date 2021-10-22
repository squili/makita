create table BotUsers (
    id      bigint  primary key,
    guilds  bigint[] not null
);

create table Sessions (
    id          bigserial   primary key,
    user_id     bigint      references BotUsers (id) on delete cascade,
    expire_at   timestamp
)
