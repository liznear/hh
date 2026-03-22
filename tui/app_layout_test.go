package tui

import "testing"

func TestComputeLayout_SidebarThreshold(t *testing.T) {
	m := &model{}

	withoutSidebar := m.computeLayout(sidebarHideWidth, 40)
	if withoutSidebar.showSidebar {
		t.Fatalf("expected sidebar hidden at width=%d", sidebarHideWidth)
	}

	withSidebar := m.computeLayout(sidebarHideWidth+1, 40)
	if !withSidebar.showSidebar {
		t.Fatalf("expected sidebar shown at width=%d", sidebarHideWidth+1)
	}
}

func TestComputeLayout_UsesConsistentInputOffsets(t *testing.T) {
	m := &model{}
	layout := m.computeLayout(180, 48)

	if !layout.valid {
		t.Fatal("expected valid layout")
	}

	if got, want := layout.inputBoxWidth, max(1, layout.mainWidth-inputBoxWidthOffset); got != want {
		t.Fatalf("unexpected inputBoxWidth: got=%d want=%d", got, want)
	}

	if got, want := layout.inputTextWidth, max(1, layout.mainWidth-inputTextWidthOffset); got != want {
		t.Fatalf("unexpected inputTextWidth: got=%d want=%d", got, want)
	}
}
