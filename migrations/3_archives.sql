-- Copyright 2021 Mia
-- This program is distributed under the terms of the GNU Affero General Public License
-- You should have received a copy of the license along with this program
-- If not, see <https://www.gnu.org/licenses/#AGPL>

create table ArchiveChannel (
    guild_id    bigint  references Guilds (id) on delete cascade,
    channel_id  bigint,
    constraint archive_idx unique (guild_id)
);

alter type PermissionType add value 'CreateArchive';
