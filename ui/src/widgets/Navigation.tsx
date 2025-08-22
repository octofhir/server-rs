import { A } from "@solidjs/router";
import { Component } from "solid-js";
import styles from "./Navigation.module.css";

const Navigation: Component = () => {
  return (
    <nav class={styles.nav}>
      <div class={styles.container}>
        <div class={styles.brand}>
          <A href="/" class={styles.brandLink}>
            OctoFHIR
          </A>
        </div>
        <div class={styles.links}>
          <A href="/" class={styles.link} activeClass={styles.activeLink} end>
            Home
          </A>
          <A href="/resources" class={styles.link} activeClass={styles.activeLink}>
            Resources
          </A>
          <A href="/console" class={styles.link} activeClass={styles.activeLink}>
            Console
          </A>
          <A href="/settings" class={styles.link} activeClass={styles.activeLink}>
            Settings
          </A>
        </div>
      </div>
    </nav>
  );
};

export default Navigation;
