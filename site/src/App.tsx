import Hero from "./components/Hero";
import WorkspaceVisual from "./components/WorkspaceVisual";
import Features from "./components/Features";
import Philosophy from "./components/Philosophy";
import Download from "./components/Download";
import Footer from "./components/Footer";

export default function App() {
  return (
    <div className="bg-bg-deep min-h-screen">
      <Hero />
      <WorkspaceVisual />
      <Features />
      <Philosophy />
      <Download />
      <Footer />
    </div>
  );
}
