import { Component, type ErrorInfo, type ReactNode, useState } from "react";
import { ArrowRotateRight, ChevronDown, ChevronUp, House, TriangleExclamation } from "@gravity-ui/icons";
import { Button } from "../Button";
import classes from "./ErrorBoundary.module.css";

interface ErrorBoundaryProps {
	children: ReactNode;
	fallback?: ReactNode;
	onError?: (error: Error, errorInfo: ErrorInfo) => void;
	layout?: boolean;
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

		if (import.meta.env.DEV) {
			console.error("ErrorBoundary caught an error:", error, errorInfo);
		}
	}

	componentDidUpdate(prevProps: ErrorBoundaryProps) {
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

	handleGoHome = () => {
		this.reset();
		window.location.assign("/ui");
	};

	render() {
		if (!this.state.hasError) {
			return this.props.children;
		}

		if (this.props.fallback) {
			return this.props.fallback;
		}

		if (this.props.layout) {
			return <LayoutErrorFallback error={this.state.error} onRetry={this.reset} />;
		}

		return (
			<GlobalErrorFallback
				error={this.state.error}
				errorInfo={this.state.errorInfo}
				onRetry={this.reset}
				onGoHome={this.handleGoHome}
			/>
		);
	}
}

interface LayoutErrorFallbackProps {
	error: Error | null;
	onRetry: () => void;
}

function LayoutErrorFallback({ error, onRetry }: LayoutErrorFallbackProps) {
	const [showDetails, setShowDetails] = useState(false);

	return (
		<section className={`${classes.shell} ${classes.layoutShell}`} role="alert">
			<div className={classes.panel}>
				<ErrorHeader
					title="Page failed to render"
					description="The current section crashed while rendering. Retry the section or move to another page."
				/>

				<div className={classes.actions}>
					<Button view="action" leftSection={<ArrowRotateRight size={16} />} onClick={onRetry}>
						Retry
					</Button>
					{error ? (
						<Button
							view="flat-secondary"
							leftSection={showDetails ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
							onClick={() => setShowDetails((value) => !value)}
						>
							Details
						</Button>
					) : null}
				</div>

				{showDetails && error ? <ErrorDetails error={error} /> : null}
			</div>
		</section>
	);
}

interface GlobalErrorFallbackProps {
	error: Error | null;
	errorInfo: ErrorInfo | null;
	onRetry: () => void;
	onGoHome: () => void;
}

function GlobalErrorFallback({ error, errorInfo, onRetry, onGoHome }: GlobalErrorFallbackProps) {
	const [showDetails, setShowDetails] = useState(import.meta.env.DEV);

	return (
		<main className={classes.shell} role="alert">
			<div className={`${classes.panel} ${classes.globalPanel}`}>
				<ErrorHeader
					title="Application error"
					description="The UI hit an unexpected state. Retry the render, or return to the main workspace."
				/>

				<div className={classes.actions}>
					<Button view="action" leftSection={<ArrowRotateRight size={16} />} onClick={onRetry}>
						Retry
					</Button>
					<Button view="outlined" leftSection={<House size={16} />} onClick={onGoHome}>
						Home
					</Button>
					{error ? (
						<Button
							view="flat-secondary"
							leftSection={showDetails ? <ChevronUp size={16} /> : <ChevronDown size={16} />}
							onClick={() => setShowDetails((value) => !value)}
						>
							Details
						</Button>
					) : null}
				</div>

				{showDetails && error ? <ErrorDetails error={error} componentStack={errorInfo?.componentStack} /> : null}
			</div>
		</main>
	);
}

function ErrorHeader({ title, description }: { title: string; description: string }) {
	return (
		<div className={classes.header}>
			<div className={classes.icon}>
				<TriangleExclamation size={24} />
			</div>
			<div className={classes.copy}>
				<h1>{title}</h1>
				<p>{description}</p>
			</div>
		</div>
	);
}

function ErrorDetails({ error, componentStack }: { error: Error; componentStack?: string | null }) {
	return (
		<div className={classes.details}>
			<div className={classes.detailHeader}>Diagnostics</div>
			<pre>{`${error.name}: ${error.message}`}</pre>
			{error.stack ? <pre>{error.stack}</pre> : null}
			{componentStack ? <pre>{componentStack}</pre> : null}
		</div>
	);
}

export default ErrorBoundary;
