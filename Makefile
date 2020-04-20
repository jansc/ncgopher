CARGO := cargo
BINARY := ncgopher
PREFIX := /usr/local
EXEC_PREFIX := ${PREFIX}
BINDIR := ${EXEC_PREFIX}/bin
DATAROOTDIR := ${PREFIX}/share
MANDIR := ${DATAROOTDIR}/man
MAN1DIR := ${MANDIR}/man1

.PHONY: build
build:
	${CARGO} build --release

.PHONY: install
install: install-bin install-man clean

.PHONY: install-man
install-man: ncgopher.1
	gzip -k ./ncgopher.1
	install -d ${DESTDIR}${MAN1DIR}
	install -m 0644 ./ncgopher.1.gz ${DESTDIR}${MAN1DIR}

.PHONY: install-bin
install-bin: build
	install -d ${DESTDIR}${BINDIR}
	install -m 0755 ./target/release/${BINARY} ${DESTDIR}${BINDIR}

.PHONY: clean
clean: 
	${CARGO} clean
	rm -f ./ncgopher.1.gz 2> /dev/null

.PHONY: uninstall
uninstall: clean
	rm -f ${DESTDIR}${MAN1DIR}/ncgopher.1.gz
	rm -f ${DESTDIR}${BINDIR}/${BINARY}


.PHONY: test
test: clean build
