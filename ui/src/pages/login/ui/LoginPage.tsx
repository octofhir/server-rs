import { createSignal, Show } from "solid-js";
import { useNavigate } from "@solidjs/router";
import { Button, Card, Input } from "@/shared/ui";
import { login, authError, clearAuthError } from "@/entities/auth";
import styles from "./LoginPage.module.css";

export const LoginPage = () => {
  const navigate = useNavigate();
  const [username, setUsername] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [isSubmitting, setIsSubmitting] = createSignal(false);

  const handleSubmit = async (e: Event) => {
    e.preventDefault();
    clearAuthError();
    setIsSubmitting(true);

    try {
      await login(username(), password());
      // Redirect to dashboard on success
      navigate("/", { replace: true });
    } catch {
      // Error is already set in store
    } finally {
      setIsSubmitting(false);
    }
  };

  const isFormValid = () => username().trim() !== "" && password().trim() !== "";

  return (
    <div class={styles.container}>
      <div class={styles.loginBox}>
        <div class={styles.header}>
          <h1 class={styles.title}>OctoFHIR</h1>
          <p class={styles.subtitle}>Sign in to continue</p>
        </div>

        <Card>
          <form onSubmit={handleSubmit} class={styles.form}>
            <Show when={authError()}>
              <div class={styles.errorBanner}>{authError()}</div>
            </Show>

            <div class={styles.formGroup}>
              <Input
                label="Username"
                type="text"
                value={username()}
                onInput={(e) => setUsername(e.currentTarget.value)}
                placeholder="Enter your username"
                autocomplete="username"
                required
                fullWidth
                disabled={isSubmitting()}
              />
            </div>

            <div class={styles.formGroup}>
              <Input
                label="Password"
                type="password"
                value={password()}
                onInput={(e) => setPassword(e.currentTarget.value)}
                placeholder="Enter your password"
                autocomplete="current-password"
                required
                fullWidth
                disabled={isSubmitting()}
              />
            </div>

            <Button
              type="submit"
              fullWidth
              loading={isSubmitting()}
              disabled={!isFormValid() || isSubmitting()}
            >
              Sign in
            </Button>
          </form>
        </Card>

        <p class={styles.footer}>
          FHIR R4B Server Console
        </p>
      </div>
    </div>
  );
};
