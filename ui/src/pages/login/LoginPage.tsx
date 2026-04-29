import { useId, useState } from "react";
import type { FormEvent } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
	Alert,
	Button,
	Card,
	Flex,
	PasswordInput,
	Text,
	TextInput,
} from "@octofhir/ui-kit";
import { useAuth } from "@/shared/api/hooks";
import classes from "./LoginPage.module.css";

const logoUrl = `${import.meta.env.BASE_URL}logo.png`;

export function LoginPage() {
	const navigate = useNavigate();
	const location = useLocation();
	const { login, loginError, isLoggingIn, refetch } = useAuth();
	const [username, setUsername] = useState("");
	const [password, setPassword] = useState("");
	const usernameId = useId();
	const passwordId = useId();

	const from =
		(location.state as { from?: { pathname: string } } | null)?.from?.pathname ??
		"/";

	const handleSubmit = async (event: FormEvent<HTMLFormElement>) => {
		event.preventDefault();
		try {
			await login({ username, password });
			await refetch();
			navigate(from, { replace: true });
		} catch {
			/* surfaced via loginError */
		}
	};

	const errorMessage =
		loginError instanceof Error ? loginError.message : loginError ? "Login failed" : null;
	const isFormValid = username.trim() !== "" && password.trim() !== "";

	return (
		<div className={classes.scene}>
			<div className={classes.glow} aria-hidden="true" />
			<div className={classes.shell}>
				<Flex direction="column" alignItems="center" gap={4} className={classes.brand}>
					<img src={logoUrl} alt="OctoFHIR" className={classes.logo} />
					<Text variant="subheader-2" color="primary">
						OctoFHIR Console
					</Text>
					<Text variant="body-2" color="secondary">
						Sign in to administer your FHIR server
					</Text>
				</Flex>

				<Card view="raised" type="container" className={classes.panel}>
					<form onSubmit={handleSubmit} className={classes.form} noValidate>
						<Flex direction="column" gap={4}>
							<Text variant="header-1">Welcome back</Text>
							<Text variant="body-2" color="secondary">
								Use your server credentials to continue.
							</Text>
						</Flex>

						{errorMessage ? (
							<Alert theme="danger" view="filled" message={errorMessage} layout="horizontal" />
						) : null}

						<Flex direction="column" gap={3}>
							<div className={classes.field}>
								<label htmlFor={usernameId}>
									<Text variant="caption-2" color="secondary">
										Username
									</Text>
								</label>
								<TextInput
									id={usernameId}
									size="l"
									placeholder="admin"
									value={username}
									onUpdate={setUsername}
									autoComplete="username"
									autoFocus
									disabled={isLoggingIn}
									hasClear
								/>
							</div>

							<div className={classes.field}>
								<label htmlFor={passwordId}>
									<Text variant="caption-2" color="secondary">
										Password
									</Text>
								</label>
								<PasswordInput
									id={passwordId}
									size="l"
									placeholder="••••••••"
									value={password}
									onUpdate={setPassword}
									autoComplete="current-password"
									disabled={isLoggingIn}
								/>
							</div>
						</Flex>

						<Button
							type="submit"
							view="action"
							size="xl"
							width="max"
							loading={isLoggingIn}
							disabled={!isFormValid || isLoggingIn}
						>
							Sign in
						</Button>

						<Text variant="caption-1" color="secondary" className={classes.footer}>
							Powered by Gravity UI
						</Text>
					</form>
				</Card>
			</div>
		</div>
	);
}
