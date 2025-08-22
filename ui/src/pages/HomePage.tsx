import { Component } from "solid-js";

const HomePage: Component = () => {
  return (
    <div>
      <h1>OctoFHIR Server</h1>
      <p>
        Welcome to the OctoFHIR Server REST Console. This tool helps you interact with FHIR
        resources, test API endpoints, and manage your FHIR server.
      </p>
      <div>
        <h2>Quick Start</h2>
        <ul>
          <li>
            <strong>Resource Browser:</strong> Browse and view FHIR resources stored in your server
          </li>
          <li>
            <strong>REST Console:</strong> Test FHIR API endpoints with a powerful request builder
          </li>
          <li>
            <strong>Settings:</strong> Configure server connection and preferences
          </li>
        </ul>
      </div>
    </div>
  );
};

export default HomePage;
