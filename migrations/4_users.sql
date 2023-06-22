-- Copyright 2021 Mia
-- This program is distributed under the terms of the GNU Affero General Public License
-- You should have received a copy of the license along with this program
-- If not, see <https://www.gnu.org/licenses/#AGPL>

create table BotUsers (
    id      bigint  primary key,
    guilds  bigint[] not null
);

create table Sessions (
    id          bigserial   primary key,
    user_id     bigint      references BotUsers (id) on delete cascade,
    expire_at   timestamp
)
