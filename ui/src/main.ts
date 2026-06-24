import "./app.css";
import "./lib/i18n";
import { mount } from "svelte";
import App from "./App.svelte";

const target = document.getElementById("app");
if (!target) throw new Error("#app mount target not found");

const app = mount(App, { target });

export default app;
