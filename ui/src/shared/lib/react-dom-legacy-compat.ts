import ReactDOM from "react-dom";

type ReactDOMWithLegacyFind = typeof ReactDOM & {
	findDOMNode?: (instance: unknown) => Element | Text | null;
};

const reactDOM = ReactDOM as ReactDOMWithLegacyFind;

if (typeof reactDOM.findDOMNode !== "function") {
	reactDOM.findDOMNode = () => null;
}
