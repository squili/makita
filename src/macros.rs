// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

#[macro_export]
macro_rules! impl_cache_functions {
    ($read_name: ident, $write_name: ident, $write_async_name: ident, $key_type: ty, $value_type: ty, $lock: ident, $default: expr) => {
        #[allow(unused)]
        pub async fn $read_name<F, R>(&self, key: &$key_type, mut func: F) -> R
        where
            F: FnMut(&$value_type) -> R,
        {
            let handle = self.$lock.read().await;
            match handle.get(&key) {
                Some(v) => func(v),
                None => {
                    drop(handle);
                    let value = $default(key);
                    let ret = func(&value);
                    let mut handle = self.$lock.write().await;
                    handle.insert(*key, value);
                    ret
                }
            }
        }

        #[allow(unused)]
        pub async fn $write_name<F, R>(&self, key: &$key_type, mut func: F) -> R
        where
            F: FnMut(&mut $value_type) -> R,
        {
            let mut handle = self.$lock.write().await;
            match handle.get_mut(&key) {
                Some(v) => func(v),
                None => {
                    let mut value = $default(key);
                    let ret = func(&mut value);
                    handle.insert(*key, value);
                    ret
                }
            }
        }

        // hey you, reading this code. don't make an issue about how ugly this signature is.
        // i spent an hour getting this to work and im not making it prettier.
        #[allow(unused)]
        pub async fn $write_async_name<'b, F, R>(&'b self, key: &$key_type, mut func: F) -> R
        where
            F: for<'a> FnMut(&'a mut $value_type, &'a &'b ()) -> futures::future::BoxFuture<'a, R>,
        {
            let mut handle = self.$lock.write().await;
            match handle.get_mut(&key) {
                Some(v) => func(v, &&()).await,
                None => {
                    let mut value = $default(key);
                    let ret = func(&mut value, &&()).await;
                    handle.insert(*key, value);
                    ret
                }
            }
        }
    };
}

#[macro_export]
macro_rules! debug {
    ($($args: expr),*) => {
        if cfg!(debug_assertions) {
            log::debug!($($args),*);
        }
    }
}

#[macro_export]
macro_rules! invite_url {
    ($id: expr) => {
        format!("https://discord.com/oauth2/authorize?client_id={}&permissions=8&scope=applications.commands+bot", $id)
    }
}

/// turns something into a string - useful shorthand
#[macro_export]
macro_rules! s {
    ($string: expr) => {
        $string.to_string()
    };
}
