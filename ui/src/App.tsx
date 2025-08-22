import { ParentComponent } from "solid-js";
import Navigation from "@/widgets/Navigation";
import styles from "./App.module.css";

const App: ParentComponent = (props) => {
  return (
    <div class={styles.app}>
      <Navigation />
      <main class={styles.main}>{props.children}</main>
    </div>
  );
};

export default App;
