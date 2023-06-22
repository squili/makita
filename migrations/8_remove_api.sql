-- Copyright 2021 Mia
-- This program is distributed under the terms of the GNU Affero General Public License
-- You should have received a copy of the license along with this program
-- If not, see <https://www.gnu.org/licenses/#AGPL>

drop table Sessions;
drop table BotUsers;
drop table Admins;

-- remove legacy permissions
delete from Permissions where type = 'WebViewer';
delete from Permissions where type = 'WebEditor';
alter type PermissionType rename value 'WebViewer' to 'Unused1';
alter type PermissionType rename value 'WebEditor' to 'Unused2';
