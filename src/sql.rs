// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

// implementations of sql stuff

use crate::utils::SqlId;
use sqlx::database::{HasArguments, HasValueRef};
use sqlx::decode::Decode;
use sqlx::encode::{Encode, IsNull};
use sqlx::error::BoxDynError;
use sqlx::postgres::PgTypeInfo;
use sqlx::{Postgres, Type};

impl<'r, T> Decode<'r, Postgres> for SqlId<T>
where
    T: From<u64> + Into<u64>,
{
    fn decode(value: <Postgres as HasValueRef<'r>>::ValueRef) -> Result<Self, BoxDynError> {
        Ok(SqlId(T::from(
            <i64 as Decode<Postgres>>::decode(value)? as u64
        )))
    }
}

impl<'r, T> Encode<'r, Postgres> for SqlId<T>
where
    T: From<u64> + Into<u64> + Copy,
{
    fn encode(self, buf: &mut <Postgres as HasArguments<'r>>::ArgumentBuffer) -> IsNull
    where
        Self: Sized,
    {
        <i64 as Encode<Postgres>>::encode(self.0.into() as i64, buf)
    }

    fn encode_by_ref(&self, buf: &mut <Postgres as HasArguments<'r>>::ArgumentBuffer) -> IsNull {
        <i64 as Encode<Postgres>>::encode_by_ref(&(self.0.into() as i64), buf)
    }

    fn produces(&self) -> Option<PgTypeInfo> {
        <i64 as Encode<Postgres>>::produces(&(self.0.into() as i64))
    }
}

impl<T> Type<Postgres> for SqlId<T>
where
    T: From<u64> + Into<u64>,
{
    fn type_info() -> PgTypeInfo {
        <i64 as Type<Postgres>>::type_info()
    }
}

impl<T> AsRef<T> for SqlId<T>
where
    T: From<u64> + Into<u64>,
{
    fn as_ref(&self) -> &T {
        &self.0
    }
}
