-- Copyright 2021 Mia
-- This program is distributed under the terms of the GNU Affero General Public License
-- You should have received a copy of the license along with this program
-- If not, see <https://www.gnu.org/licenses/#AGPL>

create type PermissionType as enum ('Administrator', 'ManagePermissions', 'ManagePreviews');

create table Permissions (
    guild_id    bigint          references Guilds (id) on delete cascade,
    type        PermissionType  not null,
    overwrites  bigint          not null,
    roles       bigint[]        not null,
    users       bigint[]        not null,
    constraint permissions_idx unique (guild_id, type)
);
