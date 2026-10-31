import * as vscode from 'vscode';
import { registerDocumentCommandGroup, type DocumentCommandContext } from '../documentCommandGroup';

function makeDependencies(): DocumentCommandContext & {
  checkSyntax: jest.Mock;
  formatDocument: jest.Mock;
  showIncPaths: jest.Mock;
  openModule: jest.Mock;
  createModule: jest.Mock;
  createTest: jest.Mock;
  showParserAst: jest.Mock;
} {
  return {
    checkSyntax: jest.fn(async () => undefined),
    formatDocument: jest.fn(async () => undefined),
    showIncPaths: jest.fn(async () => undefined),
    openModule: jest.fn(async () => undefined),
    createModule: jest.fn(async () => undefined),
    createTest: jest.fn(async () => undefined),
    showParserAst: jest.fn(async () => undefined),
  };
}

let registeredDisposables: vscode.Disposable[] = [];

afterEach(() => {
  for (const disposable of registeredDisposables) {
    disposable.dispose();
  }
  registeredDisposables = [];
});

describe('registerDocumentCommandGroup', () => {
  test('registers and delegates every document command', async () => {
    const dependencies = makeDependencies();
    registeredDisposables = registerDocumentCommandGroup(dependencies);

    expect(registeredDisposables).toHaveLength(7);
    await vscode.commands.executeCommand('perl-lsp.checkSyntax');
    await vscode.commands.executeCommand('perl-lsp.formatDocument');
    await vscode.commands.executeCommand('perl-lsp.showIncPaths');
    await vscode.commands.executeCommand('perl-lsp.openModule');
    await vscode.commands.executeCommand('perl-lsp.createModule');
    await vscode.commands.executeCommand('perl-lsp.createTest');
    await vscode.commands.executeCommand('perl-lsp.showParserAst');

    expect(dependencies.checkSyntax).toHaveBeenCalledTimes(1);
    expect(dependencies.formatDocument).toHaveBeenCalledTimes(1);
    expect(dependencies.showIncPaths).toHaveBeenCalledTimes(1);
    expect(dependencies.openModule).toHaveBeenCalledTimes(1);
    expect(dependencies.createModule).toHaveBeenCalledTimes(1);
    expect(dependencies.createTest).toHaveBeenCalledTimes(1);
    expect(dependencies.showParserAst).toHaveBeenCalledTimes(1);
  });

  test('does not invoke feature callbacks during registration', () => {
    const dependencies = makeDependencies();
    registeredDisposables = registerDocumentCommandGroup(dependencies);

    expect(dependencies.checkSyntax).not.toHaveBeenCalled();
    expect(dependencies.formatDocument).not.toHaveBeenCalled();
    expect(dependencies.showIncPaths).not.toHaveBeenCalled();
    expect(dependencies.openModule).not.toHaveBeenCalled();
    expect(dependencies.createModule).not.toHaveBeenCalled();
    expect(dependencies.createTest).not.toHaveBeenCalled();
    expect(dependencies.showParserAst).not.toHaveBeenCalled();
  });
});
