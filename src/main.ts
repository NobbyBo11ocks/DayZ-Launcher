// First, so an error while any store is built is caught (row 25).
import "./lib/errorhooks";
import { mount } from "svelte";
import "./app.css";
import App from "./App.svelte";

const target = document.getElementById("app");
if (!target) throw new Error("#app mount point missing");

export default mount(App, { target });
