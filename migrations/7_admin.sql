-- Copyright 2021 Mia
-- This program is distributed under the terms of the GNU Affero General Public License
-- You should have received a copy of the license along with this program
-- If not, see <https://www.gnu.org/licenses/#AGPL>

create table Admins (
    id                  bigint  primary key,
    manage_admins       boolean not null,
    manage_instance     boolean not null,
    bypass_permissions  boolean not null
);
