package tui

const (
	inputBoxWidthOffset  = 0
	inputTextWidthOffset = 0
)

type layoutState struct {
	valid bool

	outerWidth  int
	outerHeight int

	innerWidth  int
	innerHeight int

	showSidebar  bool
	mainWidth    int
	sidebarWidth int

	messageHeight int
	inputHeight   int

	inputBoxWidth  int
	inputTextWidth int
}

func (m *model) computeLayout(width, height int) layoutState {
	if width <= 0 || height <= 0 {
		return layoutState{}
	}

	innerW := max(1, width-(appPadding*2))
	innerH := max(1, height-(appPadding*2))
	showSidebar := width > sidebarHideWidth

	mainW := innerW
	if showSidebar {
		mainW = max(1, innerW-sidebarWidth-mainSidebarGap)
	}

	messageH, inputH := computePaneHeights(innerH)

	return layoutState{
		valid:          true,
		outerWidth:     width,
		outerHeight:    height,
		innerWidth:     innerW,
		innerHeight:    innerH,
		showSidebar:    showSidebar,
		mainWidth:      mainW,
		sidebarWidth:   sidebarWidth,
		messageHeight:  messageH,
		inputHeight:    inputH,
		inputBoxWidth:  max(1, mainW-inputBoxWidthOffset),
		inputTextWidth: max(1, mainW-inputTextWidthOffset),
	}
}

func computePaneHeights(total int) (messageHeight int, inputHeight int) {
	if total <= 2 {
		return 1, 1
	}

	input := defaultInputLines
	if total <= defaultInputLines {
		input = 1
	}

	message := total - input
	if message < 1 {
		message = 1
		input = max(1, total-message)
	}

	return message, input
}
