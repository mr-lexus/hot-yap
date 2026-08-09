import i18n from "i18next";
import { initReactI18next } from "react-i18next";
import en from "./locales/en.json";
import ru from "./locales/ru.json";

export const LANGUAGE_KEY = "hotyap-language";
const savedLanguage = window.localStorage.getItem(LANGUAGE_KEY);
const initialLanguage = savedLanguage === "ru" || savedLanguage === "en"
  ? savedLanguage
  : window.navigator.language.toLowerCase().startsWith("ru") ? "ru" : "en";

void i18n.use(initReactI18next).init({
  resources: {
    en: { translation: en },
    ru: { translation: ru },
  },
  lng: initialLanguage,
  fallbackLng: "en",
  interpolation: { escapeValue: false },
});

i18n.on("languageChanged", (language) => {
  window.localStorage.setItem(LANGUAGE_KEY, language.startsWith("ru") ? "ru" : "en");
  document.documentElement.lang = language.startsWith("ru") ? "ru" : "en";
});

document.documentElement.lang = initialLanguage;

export default i18n;
