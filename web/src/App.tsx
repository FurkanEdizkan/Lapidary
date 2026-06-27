import { TopNav } from './components/TopNav';
import { PinnedGroupsBar } from './components/PinnedGroupsBar';
import { TagRail } from './components/TagRail';
import { Gallery } from './components/Gallery';
import { DetailOverlay } from './components/DetailOverlay';
import { InspectOverlay } from './components/InspectOverlay';
import { AddModelModal } from './components/AddModelModal';
import { TagsGroupsManager } from './components/TagsGroupsManager';
import { C, F } from './theme';

export default function App() {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', height: '100vh', background: C.page, color: C.text, fontFamily: F.sans, overflow: 'hidden' }}>
      <TopNav />
      <PinnedGroupsBar />
      <TagRail />
      <Gallery />
      <DetailOverlay />
      <InspectOverlay />
      <AddModelModal />
      <TagsGroupsManager />
    </div>
  );
}
