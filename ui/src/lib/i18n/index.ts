import { addMessages, init, getLocaleFromNavigator } from "svelte-i18n";
import en from "./en.json";
import fr from "./fr.json";

// Synchronous dictionaries — no network fetch (the app runs offline in Tauri).
// Every namespace is registered up front so no key is ever missing at runtime.
addMessages("en", en);
addMessages("fr", fr);

init({
  fallbackLocale: "en",
  initialLocale: getLocaleFromNavigator(),
});
