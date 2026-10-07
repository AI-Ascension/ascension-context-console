mod config;
mod currentness;
mod database_files;
mod grants;
mod http;
mod keys;
mod principal;
mod process_lock;
mod protected_files;
mod provision;
mod routes;
mod service;
mod slots;
mod slots_startup;

#[cfg(test)]
mod tests_adapters;
#[cfg(test)]
mod tests_files;
#[cfg(test)]
mod tests_http;
#[cfg(test)]
mod tests_lock;
#[cfg(test)]
mod tests_routes;

pub(crate) fn run_cli(arguments: Vec<String>) -> Result<(), String> {
    service::run_cli(arguments)
}
