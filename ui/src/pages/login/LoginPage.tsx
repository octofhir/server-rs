import { useId, useState } from "react";
import type { FormEvent } from "react";
import { useLocation, useNavigate } from "react-router-dom";
import {
	Alert,
	Button,
	Card,
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
		(location.state as { from?: { pathname: string } } | null)?.from
			?.pathname ?? "/";

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
		loginError instanceof Error
			? loginError.message
			: loginError
				? "Login failed"
				: null;
	const isFormValid = username.trim() !== "" && password.trim() !== "";

	return (
		<div className={classes.scene}>
			<div className={classes.glow} aria-hidden="true" />
			<div className={classes.shell}>
				<div className={classes.brand}>
					<img src={logoUrl} alt="OctoFHIR" className={classes.logo} />
					<Text variant="subheader-2" color="primary">
						OctoFHIR Console
					</Text>
					<Text variant="body-2" color="secondary">
						Sign in to administer your FHIR server
					</Text>
				</div>

				<Card view="raised" className={classes.panel}>
					<form onSubmit={handleSubmit} className={classes.form} noValidate>
						<div className={classes.intro}>
							<Text variant="header-1">Welcome back</Text>
							<Text variant="body-2" color="secondary">
								Use your server credentials to continue.
							</Text>
						</div>

						{errorMessage ? (
							<Alert theme="danger" variant="filled" message={errorMessage} />
						) : null}

						<div className={classes.fields}>
							<div className={classes.field}>
								<label htmlFor={usernameId}>
									<Text variant="caption-2" color="secondary">
										Username
									</Text>
								</label>
								<TextInput
									id={usernameId}
									size="lg"
									placeholder="admin"
									value={username}
									onUpdate={setUsername}
									autoFocus
									disabled={isLoggingIn}
									{...{ autoComplete: "username" }}
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
									size="lg"
									placeholder="••••••••"
									value={password}
									onUpdate={setPassword}
									autoComplete="current-password"
									disabled={isLoggingIn}
								/>
							</div>
						</div>

						<Button
							type="submit"
							variant="filled"
							size="xl"
							width="max"
							loading={isLoggingIn}
							disabled={!isFormValid || isLoggingIn}
						>
							Sign in
						</Button>
					</form>
				</Card>
			</div>
		</div>
	);
}
