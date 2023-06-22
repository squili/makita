// Copyright 2021 Mia
// This program is distributed under the terms of the GNU Affero General Public License
// You should have received a copy of the license along with this program
// If not, see <https://www.gnu.org/licenses/#AGPL>

use anyhow::Result;
use log::LevelFilter;
use log4rs::append::console::ConsoleAppender;
use log4rs::append::rolling_file::policy::compound::roll::fixed_window::FixedWindowRoller;
use log4rs::append::rolling_file::policy::compound::trigger::size::SizeTrigger;
use log4rs::append::rolling_file::policy::compound::CompoundPolicy;
use log4rs::append::rolling_file::RollingFileAppender;
use log4rs::config::{Appender, Logger, Root};
use log4rs::encode::pattern::PatternEncoder;
use log4rs::{init_config, Config};

pub fn init(log_level_local: LevelFilter, log_level_global: LevelFilter) -> Result<()> {
    init_config(
        Config::builder()
            .appender(
                Appender::builder().build(
                    "stdout",
                    Box::new(
                        ConsoleAppender::builder()
                            .encoder(Box::new(PatternEncoder::new(
                                "{d(%m-%d %H:%M:%S.%3f)} {M} {h({l})} {m}{n}",
                            )))
                            .build(),
                    ),
                ),
            )
            .appender(
                Appender::builder().build(
                    "logfile",
                    Box::new(
                        RollingFileAppender::builder()
                            .encoder(Box::new(PatternEncoder::new(
                                "{d(%Y-%m-%d %H:%M:%S.%3f)} {M} {h({l})} {m}{n}",
                            )))
                            .build(
                                "makita.log",
                                Box::new(CompoundPolicy::new(
                                    Box::new(SizeTrigger::new(1024 * 1024)),
                                    Box::new(
                                        FixedWindowRoller::builder().build("makita.{}.log", 3)?,
                                    ),
                                )),
                            )?,
                    ),
                ),
            )
            .logger(
                Logger::builder()
                    .appender("stdout")
                    .appender("logfile")
                    .additive(false)
                    .build(env!("CARGO_PKG_NAME"), log_level_local),
            )
            .build(
                Root::builder()
                    .appender("stdout")
                    .appender("logfile")
                    .build(log_level_global),
            )?,
    )?;
    Ok(())
}
