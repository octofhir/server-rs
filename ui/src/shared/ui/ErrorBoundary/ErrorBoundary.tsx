import { Component, type ReactNode, type ErrorInfo, useState } from "react";
import {
	Stack,
	Title,
	Text,
	Button,
	Paper,
	Group,
	Code,
	Box,
	Center,
	Collapse,
	ScrollArea,
} from "@mantine/core";
import { IconAlertTriangle, IconRefresh, IconHome, IconChevronDown, IconChevronUp } from "@tabler/icons-react";

interface ErrorBoundaryProps {
	children: ReactNode;
	/** Custom fallback component */
	fallback?: ReactNode;
	/** Called when error is caught */
	onError?: (error: Error, errorInfo: ErrorInfo) => void;
	/** Whether this is a layout-level boundary (shows compact error) */
	layout?: boolean;
	/** Reset key - when this changes, the boundary resets */
	resetKey?: string | number;
}

interface ErrorBoundaryState {
	hasError: boolean;
	error: Error | null;
	errorInfo: ErrorInfo | null;
}

export class ErrorBoundary extends Component<ErrorBoundaryProps, ErrorBoundaryState> {
	constructor(props: ErrorBoundaryProps) {
		super(props);
		this.state = {
			hasError: false,
			error: null,
			errorInfo: null,
		};
	}

	static getDerivedStateFromError(error: Error): Partial<ErrorBoundaryState> {
		return { hasError: true, error };
	}

	componentDidCatch(error: Error, errorInfo: ErrorInfo) {
		this.setState({ errorInfo });
		this.props.onError?.(error, errorInfo);

		// Log to console in development
		if (import.meta.env.DEV) {
			console.error("ErrorBoundary caught an error:", error, errorInfo);
		}
	}

	componentDidUpdate(prevProps: ErrorBoundaryProps) {
		// Reset error state when resetKey changes
		if (prevProps.resetKey !== this.props.resetKey && this.state.hasError) {
			this.reset();
		}
	}

	reset = () => {
		this.setState({
			hasError: false,
			error: null,
			errorInfo: null,
		});
	};

	handleRetry = () => {
		this.reset();
	};

	handleGoHome = () => {
		this.reset();
		window.location.href = "/ui";
	};

	render() {
		if (this.state.hasError) {
			if (this.props.fallback) {
				return this.props.fallback;
			}

			const { error, errorInfo } = this.state;
			const { layout } = this.props;

			if (layout) {
				// Compact error for layout-level boundaries
				return <LayoutErrorFallback error={error} onRetry={this.handleRetry} />;
			}

			// Full-page error for global boundaries
			return (
				<GlobalErrorFallback
					error={error}
					errorInfo={errorInfo}
					onRetry={this.handleRetry}
					onGoHome={this.handleGoHome}
				/>
			);
		}

		return this.props.children;
	}
}

interface LayoutErrorFallbackProps {
	error: Error | null;
	onRetry: () => void;
}

function LayoutErrorFallback({ error, onRetry }: LayoutErrorFallbackProps) {
	const [showDetails, setShowDetails] = useState(false);

	return (
		<Center h="100%" p="xl" className="page-enter">
			<Paper
				p="xl"
				radius="lg"
				style={{
					background: "var(--app-surface-2)",
					border: "1px solid var(--app-border-subtle)",
					maxWidth: 600,
					width: "100%",
				}}
			>
				<Stack gap="md" align="center">
					<Box
						p="md"
						style={{
							background: "var(--mantine-color-red-light)",
							borderRadius: "var(--mantine-radius-lg)",
						}}
					>
						<IconAlertTriangle size={32} color="var(--mantine-color-red-filled)" />
					</Box>

					<Stack gap="xs" align="center">
						<Title order={4}>Something went wrong</Title>
						<Text size="sm" c="dimmed" ta="center">
							An error occurred while rendering this page. Try refreshing or navigate to another section.
						</Text>
					</Stack>

					<Group gap="sm">
						<Button
							leftSection={<IconRefresh size={16} />}
							onClick={onRetry}
							variant="light"
							radius="md"
						>
							Try Again
						</Button>
						{error && (
							<Button
								leftSection={showDetails ? <IconChevronUp size={16} /> : <IconChevronDown size={16} />}
								onClick={() => setShowDetails((v) => !v)}
								variant="subtle"
								radius="md"
								color="gray"
							>
								{showDetails ? "Hide Details" : "Show Details"}
							</Button>
						)}
					</Group>

					{error && (
						<Collapse in={showDetails} w="100%">
							<Stack gap="xs" w="100%">
								<Text size="xs" fw={600} tt="uppercase" c="dimmed">
									Error Details
								</Text>
								<Code
									block
									style={{
										fontSize: "11px",
										background: "var(--app-surface-3)",
										border: "1px solid var(--app-border-subtle)",
									}}
								>
									{error.name}: {error.message}
								</Code>
								{error.stack && (
									<ScrollArea h={150}>
										<Code
											block
											style={{
												fontSize: "10px",
												background: "var(--app-surface-3)",
												border: "1px solid var(--app-border-subtle)",
											}}
										>
											{error.stack}
										</Code>
									</ScrollArea>
								)}
							</Stack>
						</Collapse>
					)}
				</Stack>
			</Paper>
		</Center>
	);
}

interface GlobalErrorFallbackProps {
	error: Error | null;
	errorInfo: ErrorInfo | null;
	onRetry: () => void;
	onGoHome: () => void;
}

function GlobalErrorFallback({ error, errorInfo, onRetry, onGoHome }: GlobalErrorFallbackProps) {
	return (
		<Box
			style={{
				minHeight: "100vh",
				display: "flex",
				alignItems: "center",
				justifyContent: "center",
				background: "var(--mantine-color-body)",
				padding: "var(--mantine-spacing-xl)",
			}}
		>
			<Paper
				p="xl"
				radius="lg"
				shadow="lg"
				style={{
					maxWidth: 600,
					width: "100%",
					border: "1px solid var(--mantine-color-default-border)",
				}}
			>
				<Stack gap="lg" align="center">
					<Box
						p="lg"
						style={{
							background: "var(--mantine-color-red-light)",
							borderRadius: "var(--mantine-radius-xl)",
						}}
					>
						<IconAlertTriangle size={48} color="var(--mantine-color-red-filled)" />
					</Box>

					<Stack gap="xs" align="center">
						<Title order={2}>Application Error</Title>
						<Text c="dimmed" ta="center">
							The application encountered an unexpected error. Please try refreshing the page or return to the home screen.
						</Text>
					</Stack>

					{error && import.meta.env.DEV && (
						<Stack gap="xs" w="100%">
							<Text size="xs" fw={600} tt="uppercase" c="dimmed">
								Error Details (Development Only)
							</Text>
							<Code
								block
								style={{
									maxHeight: 150,
									overflow: "auto",
									fontSize: "11px",
								}}
							>
								{error.name}: {error.message}
							</Code>
							{errorInfo?.componentStack && (
								<Code
									block
									style={{
										maxHeight: 200,
										overflow: "auto",
										fontSize: "10px",
									}}
								>
									{errorInfo.componentStack}
								</Code>
							)}
						</Stack>
					)}

					<Group gap="md">
						<Button
							leftSection={<IconRefresh size={16} />}
							onClick={onRetry}
							variant="light"
							radius="md"
						>
							Try Again
						</Button>
						<Button
							leftSection={<IconHome size={16} />}
							onClick={onGoHome}
							radius="md"
						>
							Go to Home
						</Button>
					</Group>
				</Stack>
			</Paper>
		</Box>
	);
}

export default ErrorBoundary;
