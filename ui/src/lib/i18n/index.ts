import { addMessages, init, getLocaleFromNavigator } from "svelte-i18n";
import en from "./en.json";
import fr from "./fr.json";

// Synchronous dictionaries — no network fetch (the app runs offline in Tauri).
// Every namespace is registered up front so no key is ever missing at runtime.
addMessages("en", en);
addMessages("fr", fr);

type SupportedLocale = "en" | "fr";

/**
 * Maps an operating-system locale to one of the dictionaries bundled with the
 * application. Desktop environments commonly report values such as `fr-FR`
 * or `fr_FR.UTF-8`, while svelte-i18n only has the short `fr` and `en` keys.
 */
function resolveInitialLocale(
  locale: string | null | undefined,
): SupportedLocale {
  const language = locale
    ?.trim()
    .toLowerCase()
    .split(/[-_.@]/, 1)[0];

  return language === "fr" ? "fr" : "en";
}

init({
  fallbackLocale: "en",
  initialLocale: resolveInitialLocale(getLocaleFromNavigator()),
});
