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
	useMantineColorScheme,
} from "@mantine/core";
import { IconAlertCircle } from "@tabler/icons-react";
import { useAuth } from "@/shared/api/hooks";

export function LoginPage() {
	const navigate = useNavigate();
	const location = useLocation();
	const { login, loginError, isLoggingIn, refetch } = useAuth();
	const { colorScheme } = useMantineColorScheme();

	const [username, setUsername] = useState("");
	const [password, setPassword] = useState("");

	// Get the redirect path from location state, default to dashboard
	const from = (location.state as { from?: { pathname: string } })?.from?.pathname || "/";

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

	// Use appropriate background for light/dark mode
	const bgColor = colorScheme === "dark" ? "var(--mantine-color-dark-8)" : "var(--mantine-color-gray-0)";

	return (
		<Center h="100vh" bg={bgColor}>
			<Box w={400} mx="auto">
				<Stack align="center" mb="xl">
					<Text size="3rem">🐙</Text>
					<Title order={1}>OctoFHIR</Title>
					<Text c="dimmed">Sign in to continue</Text>
				</Stack>

				<Card shadow="sm" padding="xl" radius="md" withBorder>
					<form onSubmit={handleSubmit}>
						<Stack gap="md">
							{loginError && (
								<Alert
									icon={<IconAlertCircle size={16} />}
									color="red"
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
								value={username}
								onChange={(e) => setUsername(e.currentTarget.value)}
								autoComplete="username"
								required
								disabled={isLoggingIn}
							/>

							<PasswordInput
								label="Password"
								placeholder="Enter your password"
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
						</Stack>
					</form>
				</Card>

				<Text ta="center" c="dimmed" size="sm" mt="md">
					FHIR R4B Server Console
				</Text>
			</Box>
		</Center>
	);
}

