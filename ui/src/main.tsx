/* @refresh reload */
import { render } from "solid-js/web";
import App from "@/app";
import "@/shared/styles/global.css";

const root = document.getElementById("root");

if (!root) {
  throw new Error("Root element not found");
}

render(() => <App />, root);
