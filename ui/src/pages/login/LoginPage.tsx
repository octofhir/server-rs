import { useState } from "react";
import { useNavigate, useLocation } from "react-router-dom";
import {
	Center,
	Stack,
	Card,
	Title,
	Text,
	TextInput,
	PasswordInput,
	Button,
	Alert,
	Box,
} from "@/shared/ui";
import { IconAlertCircle } from "@tabler/icons-react";
import { useAuth } from "@/shared/api/hooks";

export function LoginPage() {
	const navigate = useNavigate();
	const location = useLocation();
	const { login, loginError, isLoggingIn, refetch } = useAuth();
	const [username, setUsername] = useState("");
	const [password, setPassword] = useState("");

	// Get the redirect path from location state, default to dashboard
	const from =
		(location.state as { from?: { pathname: string } })?.from?.pathname || "/";

	const handleSubmit = async (e: React.FormEvent) => {
		e.preventDefault();

		try {
			// Wait for login to complete
			await login({ username, password });

			// Wait for user query to refetch and complete before navigating
			// This ensures RouteGuard has the updated auth state
			await refetch();

			navigate(from, { replace: true });
		} catch {
			// Error is handled by the mutation
		}
	};

	const isFormValid = username.trim() !== "" && password.trim() !== "";

	const bgColor = "var(--app-login-bg)";
	const panelBackground = "var(--app-login-panel-bg)";

	const logoUrl = `${import.meta.env.BASE_URL}logo.png`;

	return (
		<Center h="100vh" style={{ background: bgColor }}>
			<Box w={420} mx="auto" px="md">
				<Stack align="center" mb="lg">
					<img src={logoUrl} alt="OctoFHIR logo" width={88} height={88} />
					<Title order={1}>OctoFHIR</Title>
					<Text size="sm" c="dimmed">
						Server Console
					</Text>
				</Stack>

				<Card
					padding="xl"
					radius="lg"
					style={{
						backgroundColor: panelBackground,
						backdropFilter: "blur(8px)",
					}}
				>
					<form onSubmit={handleSubmit}>
						<Stack gap="md">
							{loginError && (
								<Alert
									icon={<IconAlertCircle size={16} />}
									color="fire"
									variant="light"
								>
									{loginError instanceof Error
										? loginError.message
										: "Login failed"}
								</Alert>
							)}

							<TextInput
								label="Username"
								placeholder="Enter your username"
								size="sm"
								value={username}
								onChange={(e) => setUsername(e.currentTarget.value)}
								autoComplete="username"
								required
								disabled={isLoggingIn}
							/>

							<PasswordInput
								label="Password"
								placeholder="Enter your password"
								size="sm"
								value={password}
								onChange={(e) => setPassword(e.currentTarget.value)}
								autoComplete="current-password"
								required
								disabled={isLoggingIn}
							/>

							<Button
								type="submit"
								fullWidth
								loading={isLoggingIn}
								disabled={!isFormValid || isLoggingIn}
								mt="sm"
							>
								Sign in
							</Button>
							<Text size="xs" c="dimmed" ta="center">
								Use your server credentials to continue
							</Text>
						</Stack>
					</form>
				</Card>
			</Box>
		</Center>
	);
}
