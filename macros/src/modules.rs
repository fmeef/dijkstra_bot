/*
 * KLUDGE ALERT!
 *
 * Python weenies on telegram DEMAND that "rust scripts" be loaded
 * dynamically by the compiler without having to mess around with
 * the module tree. This is a horrible mindset but I am unable to change
 * this. Instead, I use proc macros to autogenerate mod statements for
 * any "plugins" that might exist in the bobot_impl/src/modules directory
 *
 * Unfortunately...
 *
 * Cargo has no idea what a "file path" during compilation.
 * Since we autogenerate mod statements for plugins/modules this
 * requires that cargo is aware of the relative path to the "modules"
 * directory. (in the bobot_impl crate)
 *
 * This causes a CRY because modules is in a sub-crate
 * and rust-analyzer builds from the context of the sub-crate not the
 * parent crate, My temporary workaround is to glob for anything named "modules"
 *
 * This file is here to prevent accidentally created plugins/modules from
 * being loaded in the wrong crate. For actual modules look in
 * bobot_impl::modules
 *
 * TODO: completely refactor this or somehow implement luxury
 * gay space communism to fund univeral mandated socialized higher education
 * system that trains people to actually learn rust
 */
